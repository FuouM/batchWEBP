//! Parallel folder-to-WebP conversion.
//!
//! One walker streams image paths into a queue; one worker per core decodes, encodes (libwebp,
//! single-threaded per image) and writes. Writes are atomic (temp file + rename), outputs keep the
//! source mtime, and existing outputs are skipped, so an interrupted run resumes where it stopped.

mod decode;
mod encode;

use std::ffi::OsString;
use std::fs;
use std::io::Write;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering::Relaxed};
use std::time::SystemTime;

use anyhow::{Context, bail};
use serde::{Deserialize, Serialize};

use decode::Decoded;
use encode::Encoded;
pub use encode::{Encoding, Mode};

/// Lowercase source extensions picked up by the walker. `.webp` sources are never touched.
pub const EXTENSIONS: &[&str] = &["png", "jpg", "jpeg", "jfif", "jpe", "bmp", "dib", "gif", "tif", "tiff", "tga", "qoi"];

/// Errors kept for display; the counter keeps counting past this.
const MAX_ERRORS: usize = 10_000;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Options {
    pub input: PathBuf,
    /// Mirror of the input tree. `None`: in place, each WebP replaces its original.
    pub output: Option<PathBuf>,
    pub encoding: Encoding,
    pub recursive: bool,
    /// Keep the original when the WebP is not smaller (copied into the output folder; untouched in place).
    pub keep_smaller_original: bool,
    /// Replace existing outputs in an output folder. In place never overwrites: `a.png` and `a.jpg`
    /// both map to `a.webp`, and overwriting would delete one of them.
    pub overwrite: bool,
    /// Worker threads; 0 = one per logical core.
    pub threads: usize,
}

impl Options {
    pub fn new(input: PathBuf) -> Self {
        let output = default_output(&input);
        Self {
            input,
            output: Some(output),
            encoding: Encoding::default(),
            recursive: true,
            keep_smaller_original: true,
            overwrite: false,
            threads: 0,
        }
    }
}

/// `<input>_webp` next to the input folder.
pub fn default_output(input: &Path) -> PathBuf {
    let mut name = input.file_name().map(OsString::from).unwrap_or_else(|| OsString::from("images"));
    name.push("_webp");
    input.with_file_name(name)
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileError {
    pub path: String,
    pub error: String,
}

/// Live counters, shared between the workers and whoever reports progress.
#[derive(Default)]
pub struct Progress {
    scanning: AtomicBool,
    found: AtomicU64,
    converted: AtomicU64,
    kept: AtomicU64,
    skipped: AtomicU64,
    failed: AtomicU64,
    bytes_in: AtomicU64,
    bytes_out: AtomicU64,
    errors: Mutex<Vec<FileError>>,
}

#[derive(Clone, Copy, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    /// The walker is still discovering files; `found` will grow.
    pub scanning: bool,
    pub found: u64,
    pub done: u64,
    pub converted: u64,
    /// Original kept: WebP not smaller, animated, >8-bit in lossless mode, over 16383 px, or too
    /// large for lossy VP8 (partition 0 overflow).
    pub kept: u64,
    /// Output already existed.
    pub skipped: u64,
    pub failed: u64,
    /// Source bytes of converted files.
    pub bytes_in: u64,
    /// WebP bytes of converted files.
    pub bytes_out: u64,
}

impl Progress {
    pub fn snapshot(&self) -> Snapshot {
        let (converted, kept, skipped, failed) =
            (self.converted.load(Relaxed), self.kept.load(Relaxed), self.skipped.load(Relaxed), self.failed.load(Relaxed));
        Snapshot {
            scanning: self.scanning.load(Relaxed),
            found: self.found.load(Relaxed),
            done: converted + kept + skipped + failed,
            converted,
            kept,
            skipped,
            failed,
            bytes_in: self.bytes_in.load(Relaxed),
            bytes_out: self.bytes_out.load(Relaxed),
        }
    }

    /// Errors recorded after the first `from`, for incremental polling.
    pub fn errors_since(&self, from: usize) -> Vec<FileError> {
        let errors = self.errors.lock().unwrap_or_else(|e| e.into_inner());
        errors.get(from..).map(<[FileError]>::to_vec).unwrap_or_default()
    }

    fn fail(&self, path: &Path, error: String) {
        self.failed.fetch_add(1, Relaxed);
        let mut errors = self.errors.lock().unwrap_or_else(|e| e.into_inner());
        if errors.len() < MAX_ERRORS {
            errors.push(FileError { path: path.display().to_string(), error });
        }
    }
}

enum Outcome {
    Converted { bytes_in: u64, bytes_out: u64 },
    Kept,
    Skipped,
}

/// Converts every image under `opts.input`. Returns when all files are done or `cancel` is set;
/// per-file failures land in `progress`, only setup errors are returned.
pub fn run(opts: &Options, cancel: &AtomicBool, progress: &Progress) -> anyhow::Result<()> {
    let input = opts.input.as_path();
    if !input.is_dir() {
        bail!("Input is not a folder: {}", input.display());
    }
    if let Some(output) = &opts.output {
        if fs::canonicalize(output).ok().as_deref() == Some(&*fs::canonicalize(input)?) {
            bail!("Output folder is the input folder; choose In place instead");
        }
        fs::create_dir_all(output).with_context(|| format!("Cannot create {}", output.display()))?;
    }
    let threads = match opts.threads {
        0 => std::thread::available_parallelism().map_or(4, |n| n.get()),
        n => n,
    };

    progress.scanning.store(true, Relaxed);
    // Unbounded: the walk finishes in seconds and gives an accurate total; a path is ~100 bytes.
    let (tx, rx) = crossbeam_channel::unbounded::<PathBuf>();
    std::thread::scope(|scope| {
        scope.spawn(move || {
            walk(opts, cancel, progress, tx);
            progress.scanning.store(false, Relaxed);
        });
        for _ in 0..threads {
            let rx = rx.clone();
            scope.spawn(move || {
                for path in rx {
                    if cancel.load(Relaxed) {
                        break;
                    }
                    match catch_unwind(AssertUnwindSafe(|| convert(&path, opts))) {
                        Ok(Ok(Outcome::Converted { bytes_in, bytes_out })) => {
                            progress.bytes_in.fetch_add(bytes_in, Relaxed);
                            progress.bytes_out.fetch_add(bytes_out, Relaxed);
                            progress.converted.fetch_add(1, Relaxed);
                        }
                        Ok(Ok(Outcome::Kept)) => _ = progress.kept.fetch_add(1, Relaxed),
                        Ok(Ok(Outcome::Skipped)) => _ = progress.skipped.fetch_add(1, Relaxed),
                        Ok(Err(e)) => progress.fail(&path, format!("{e:#}")),
                        Err(panic) => progress.fail(&path, format!("decoder panicked: {}", panic_message(&*panic))),
                    }
                }
            });
        }
    });
    progress.scanning.store(false, Relaxed);
    Ok(())
}

fn walk(opts: &Options, cancel: &AtomicBool, progress: &Progress, tx: crossbeam_channel::Sender<PathBuf>) {
    let output = opts.output.clone();
    ignore::WalkBuilder::new(&opts.input)
        .standard_filters(false)
        .follow_links(false)
        .max_depth(if opts.recursive { None } else { Some(1) })
        .threads(2)
        // An output folder inside the input must not feed its own WebPs (or kept copies) back in.
        .filter_entry(move |e| output.as_deref() != Some(e.path()))
        .build_parallel()
        .run(|| {
            let tx = tx.clone();
            Box::new(move |entry| {
                if cancel.load(Relaxed) {
                    return ignore::WalkState::Quit;
                }
                match entry {
                    Ok(e) if e.file_type().is_some_and(|t| t.is_file()) && is_image(e.path()) => {
                        progress.found.fetch_add(1, Relaxed);
                        // Receivers outlive the walker; a send error only means every worker stopped.
                        if tx.send(e.into_path()).is_err() {
                            return ignore::WalkState::Quit;
                        }
                    }
                    Ok(_) => {}
                    Err(err) => progress.fail(&walk_error_path(&err), format!("{err}")),
                }
                ignore::WalkState::Continue
            })
        });
}

fn walk_error_path(err: &ignore::Error) -> PathBuf {
    match err {
        ignore::Error::WithPath { path, .. } => path.clone(),
        ignore::Error::WithDepth { err, .. } | ignore::Error::WithLineNumber { err, .. } => walk_error_path(err),
        _ => PathBuf::from("<walk>"),
    }
}

pub fn is_image(path: &Path) -> bool {
    path.extension().and_then(|e| e.to_str()).is_some_and(|e| EXTENSIONS.iter().any(|x| x.eq_ignore_ascii_case(e)))
}

fn convert(src: &Path, opts: &Options) -> anyhow::Result<Outcome> {
    let rel = src.strip_prefix(&opts.input).unwrap_or(src);
    let target = match &opts.output {
        Some(out) => out.join(rel).with_extension("webp"),
        None => src.with_extension("webp"),
    };
    let in_place = opts.output.is_none();
    if (in_place || !opts.overwrite) && target.exists() {
        return Ok(Outcome::Skipped);
    }
    // A kept original copied by an earlier run: resuming must not decode and encode it again.
    if let Some(out) = &opts.output
        && !opts.overwrite
        && out.join(rel).exists()
    {
        return Ok(Outcome::Skipped);
    }

    let data = fs::read(src).context("read")?;
    let mtime = fs::metadata(src).and_then(|m| m.modified()).ok();
    let webp = match encode_bytes(&data, &opts.encoding)? {
        Some(webp) if !(opts.keep_smaller_original && webp.len() >= data.len()) => Some(webp),
        _ => None,
    };

    let Some(webp) = webp else {
        // The output folder stays a complete mirror: the original goes there unchanged.
        if let Some(out) = &opts.output {
            let dest = out.join(rel);
            if opts.overwrite || !dest.exists() {
                write_atomic(&dest, &data, mtime).context("copy original")?;
            }
        }
        return Ok(Outcome::Kept);
    };

    write_atomic(&target, &webp, mtime).context("write")?;
    if in_place {
        fs::remove_file(src).context("WebP written, but deleting the original failed")?;
    }
    Ok(Outcome::Converted { bytes_in: data.len() as u64, bytes_out: webp.len() as u64 })
}

/// Encoded image file -> WebP bytes. `None`: WebP cannot hold it as asked (animated, over 16383 px,
/// more than 8 bits per channel in lossless mode, lossy partition 0 overflow); keep the original.
pub fn encode_bytes(data: &[u8], encoding: &Encoding) -> anyhow::Result<Option<Vec<u8>>> {
    match decode::decode(data).context("decode")? {
        Decoded::Still(img) => match encode::encode(&img, encoding).map_err(anyhow::Error::msg)? {
            Encoded::Webp(webp) => Ok(Some(webp)),
            Encoded::Keep => Ok(None),
        },
        Decoded::Keep => Ok(None),
    }
}

/// Temp file + rename: a crash never leaves a truncated file under the final name.
fn write_atomic(path: &Path, bytes: &[u8], mtime: Option<SystemTime>) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".batchwebp-tmp");
    let tmp = PathBuf::from(tmp);
    let result = (|| {
        let mut file = fs::File::create(&tmp)?;
        file.write_all(bytes)?;
        if let Some(mtime) = mtime {
            file.set_modified(mtime)?;
        }
        drop(file);
        fs::rename(&tmp, path)
    })();
    if result.is_err() {
        _ = fs::remove_file(&tmp);
    }
    result
}

fn panic_message(panic: &(dyn std::any::Any + Send)) -> &str {
    panic.downcast_ref::<&str>().copied().or_else(|| panic.downcast_ref::<String>().map(String::as_str)).unwrap_or("unknown panic")
}
