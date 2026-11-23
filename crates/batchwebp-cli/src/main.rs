use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant};

use batchwebp_core::{Encoding, Mode, Options, Progress, Snapshot};
use clap::{Parser, ValueEnum};

/// Convert a folder of images to WebP.
#[derive(Parser)]
#[command(name = "batchwebp", version)]
struct Cli {
    /// Folder to convert.
    input: PathBuf,
    /// Output folder (default: <input>_webp next to the input).
    #[arg(short, long, conflicts_with = "in_place")]
    output: Option<PathBuf>,
    /// Replace each original with its WebP instead of writing an output folder.
    #[arg(long)]
    in_place: bool,
    #[arg(short, long, value_enum, default_value_t = CliMode::Lossless)]
    mode: CliMode,
    /// 0 (fastest) ..= 9 (smallest). Measured on 2-4 MP PNGs: 0 = 19% smaller at 45 img/s, 1 = 27% at 8 img/s.
    #[arg(short, long, default_value_t = 0, value_parser = clap::value_parser!(u8).range(0..=9))]
    effort: u8,
    /// Near-lossless strength, 0 (strongest) ..= 100 (off).
    #[arg(long, default_value_t = 60, value_parser = clap::value_parser!(u8).range(0..=100))]
    near: u8,
    /// Lossy quality.
    #[arg(short, long, default_value_t = 90, value_parser = clap::value_parser!(u8).range(0..=100))]
    quality: u8,
    /// Only the top folder.
    #[arg(long)]
    no_recursive: bool,
    /// Write the WebP even when it is larger than the original.
    #[arg(long)]
    always_webp: bool,
    /// Replace existing outputs (output folder only).
    #[arg(long)]
    overwrite: bool,
    /// Worker threads (0 = all cores).
    #[arg(short = 'j', long, default_value_t = 0)]
    threads: usize,
}

#[derive(Clone, Copy, ValueEnum)]
enum CliMode {
    Lossless,
    Near,
    Lossy,
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let mode = match cli.mode {
        CliMode::Lossless => Mode::Lossless,
        CliMode::Near => Mode::NearLossless,
        CliMode::Lossy => Mode::Lossy,
    };
    let output = if cli.in_place { None } else { Some(cli.output.unwrap_or_else(|| batchwebp_core::default_output(&cli.input))) };
    let opts = Options {
        input: cli.input,
        output,
        encoding: Encoding { mode, effort: cli.effort, near_lossless: cli.near, quality: cli.quality },
        recursive: !cli.no_recursive,
        keep_smaller_original: !cli.always_webp,
        overwrite: cli.overwrite,
        threads: cli.threads,
    };

    let cancel = AtomicBool::new(false);
    let progress = Progress::default();
    let start = Instant::now();
    let result = std::thread::scope(|scope| {
        let job = scope.spawn(|| batchwebp_core::run(&opts, &cancel, &progress));
        while !job.is_finished() {
            std::thread::sleep(Duration::from_millis(250));
            print_line(&progress.snapshot(), start.elapsed());
        }
        job.join().expect("converter thread panicked")
    });
    let s = progress.snapshot();
    print_line(&s, start.elapsed());
    eprintln!();
    for e in progress.errors_since(0) {
        eprintln!("FAILED {}: {}", e.path, e.error);
    }
    result?;
    if s.failed > 0 {
        std::process::exit(1);
    }
    Ok(())
}

fn print_line(s: &Snapshot, elapsed: Duration) {
    let rate = s.done as f64 / elapsed.as_secs_f64().max(1e-3);
    let saved = if s.bytes_in > 0 { 100.0 * (1.0 - s.bytes_out as f64 / s.bytes_in as f64) } else { 0.0 };
    let total = if s.scanning { format!("{}+", s.found) } else { s.found.to_string() };
    eprint!(
        "\r{}/{} | converted {} kept {} skipped {} failed {} | {} -> {} ({saved:.1}% saved) | {rate:.0} img/s   ",
        s.done,
        total,
        s.converted,
        s.kept,
        s.skipped,
        s.failed,
        mib(s.bytes_in),
        mib(s.bytes_out),
    );
    _ = std::io::stderr().flush();
}

fn mib(bytes: u64) -> String {
    format!("{:.1} MiB", bytes as f64 / (1024.0 * 1024.0))
}
