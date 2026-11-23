//! Tauri shell: starts one conversion job at a time on a background thread; the UI polls `status`.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering::Relaxed};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use batchwebp_core::{FileError, Options, Progress, Snapshot};
use serde::Serialize;
use tauri::State;

struct Job {
    cancel: AtomicBool,
    progress: Progress,
    started: Instant,
    /// Set when the run returns: elapsed time and the setup error, if any.
    finished: Mutex<Option<(Duration, Option<String>)>>,
}

#[derive(Default)]
struct AppState(Mutex<Option<Arc<Job>>>);

impl AppState {
    fn job(&self) -> Option<Arc<Job>> {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Status {
    running: bool,
    cancelled: bool,
    elapsed_ms: u64,
    progress: Snapshot,
    /// Failures after the first `errors_from` the caller already has.
    errors: Vec<FileError>,
    /// The run could not start (bad input or output folder).
    error: Option<String>,
}

#[tauri::command]
fn start(state: State<'_, AppState>, options: Options) -> Result<(), String> {
    let mut slot = state.0.lock().unwrap_or_else(|e| e.into_inner());
    if slot.as_ref().is_some_and(|job| job.finished.lock().unwrap_or_else(|e| e.into_inner()).is_none()) {
        return Err("A conversion is already running".into());
    }
    let job = Arc::new(Job {
        cancel: AtomicBool::new(false),
        progress: Progress::default(),
        started: Instant::now(),
        finished: Mutex::new(None),
    });
    *slot = Some(job.clone());
    std::thread::Builder::new()
        .name("batchwebp-run".into())
        .spawn(move || {
            let result = batchwebp_core::run(&options, &job.cancel, &job.progress);
            *job.finished.lock().unwrap_or_else(|e| e.into_inner()) = Some((job.started.elapsed(), result.err().map(|e| format!("{e:#}"))));
        })
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
fn cancel(state: State<'_, AppState>) {
    if let Some(job) = state.job() {
        job.cancel.store(true, Relaxed);
    }
}

#[tauri::command]
fn status(state: State<'_, AppState>, errors_from: usize) -> Option<Status> {
    let job = state.job()?;
    let finished = job.finished.lock().unwrap_or_else(|e| e.into_inner()).clone();
    let (running, elapsed, error) = match finished {
        Some((elapsed, error)) => (false, elapsed, error),
        None => (true, job.started.elapsed(), None),
    };
    Some(Status {
        running,
        cancelled: job.cancel.load(Relaxed),
        elapsed_ms: elapsed.as_millis() as u64,
        progress: job.progress.snapshot(),
        errors: job.progress.errors_since(errors_from),
        error,
    })
}

#[tauri::command]
fn default_output(input: PathBuf) -> PathBuf {
    batchwebp_core::default_output(&input)
}

/// A dropped path as a folder to convert: the path itself, or the folder holding a dropped file.
#[tauri::command]
fn folder_of(path: PathBuf) -> PathBuf {
    if path.is_dir() { path } else { path.parent().map(Path::to_path_buf).unwrap_or(path) }
}

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(AppState::default())
        .invoke_handler(tauri::generate_handler![start, cancel, status, default_output, folder_of])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
