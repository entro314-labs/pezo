//! Sequential install queue. Heavy operations (extraction, adoption copies,
//! installers) fight over the same disks, so they run one at a time on a
//! worker thread. The UI mirrors the queue through `queue-changed` events,
//! which carry the full task list on every transition.

use std::collections::HashSet;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Emitter};

use pezo_core::install::{InstallOptions, InstallStage};
use pezo_core::{Config, Game, install, open_library};

#[derive(Clone, Serialize)]
pub struct QueueTask {
    pub id: u64,
    pub candidate_id: i64,
    pub title: String,
    pub archive: Option<String>,
    /// queued | running | done | failed | cancelled
    pub status: &'static str,
    pub stage: Option<&'static str>,
    /// 0..1 while bytes are landing in the library and the total is known.
    pub progress: Option<f32>,
    pub error: Option<String>,
    pub game_id: Option<i64>,
}

struct Queue {
    tasks: Mutex<Vec<QueueTask>>,
    wake: Condvar,
    next_id: AtomicU64,
}

static QUEUE: OnceLock<Arc<Queue>> = OnceLock::new();

fn queue() -> &'static Arc<Queue> {
    QUEUE.get_or_init(|| {
        Arc::new(Queue {
            tasks: Mutex::new(Vec::new()),
            wake: Condvar::new(),
            next_id: AtomicU64::new(1),
        })
    })
}

pub fn snapshot() -> Vec<QueueTask> {
    queue().tasks.lock().unwrap().clone()
}

fn emit(app: &AppHandle) {
    let _ = app.emit("queue-changed", snapshot());
}

fn with_task(id: u64, mutate: impl FnOnce(&mut QueueTask)) {
    let mut tasks = queue().tasks.lock().unwrap();
    if let Some(task) = tasks.iter_mut().find(|t| t.id == id) {
        mutate(task);
    }
}

/// Add an install to the queue. A candidate already queued or running is not
/// queued twice — the existing task id is returned instead.
pub fn enqueue(app: &AppHandle, candidate_id: i64, title: String, archive: Option<String>) -> u64 {
    let q = queue();
    let id = {
        let mut tasks = q.tasks.lock().unwrap();
        if let Some(existing) = tasks
            .iter()
            .find(|t| t.candidate_id == candidate_id && matches!(t.status, "queued" | "running"))
        {
            return existing.id;
        }
        let id = q.next_id.fetch_add(1, Ordering::Relaxed);
        tasks.push(QueueTask {
            id,
            candidate_id,
            title,
            archive,
            status: "queued",
            stage: None,
            progress: None,
            error: None,
            game_id: None,
        });
        id
    };
    q.wake.notify_one();
    emit(app);
    id
}

/// Cancel a task that hasn't started. Running tasks finish — interrupting an
/// extractor halfway leaves debris the error paths are built to avoid.
pub fn cancel(app: &AppHandle, task_id: u64) -> bool {
    let mut cancelled = false;
    with_task(task_id, |t| {
        if t.status == "queued" {
            t.status = "cancelled";
            cancelled = true;
        }
    });
    if cancelled {
        emit(app);
    }
    cancelled
}

pub fn clear_finished(app: &AppHandle) {
    queue()
        .tasks
        .lock()
        .unwrap()
        .retain(|t| matches!(t.status, "queued" | "running"));
    emit(app);
}

/// Start the worker. Called once from Tauri's setup hook.
pub fn spawn_worker(app: AppHandle) {
    std::thread::Builder::new()
        .name("install-queue".into())
        .spawn(move || {
            let q = queue().clone();
            loop {
                let task = {
                    let mut tasks = q.tasks.lock().unwrap();
                    loop {
                        if let Some(task) = tasks.iter_mut().find(|t| t.status == "queued") {
                            task.status = "running";
                            break task.clone();
                        }
                        tasks = q.wake.wait(tasks).unwrap();
                    }
                };
                emit(&app);
                let result = run_task(&app, &task);
                with_task(task.id, |t| match &result {
                    Ok(game) => {
                        t.status = "done";
                        t.stage = None;
                        t.progress = None;
                        t.game_id = Some(game.id);
                    }
                    Err(e) => {
                        t.status = "failed";
                        t.stage = None;
                        t.progress = None;
                        t.error = Some(e.clone());
                    }
                });
                emit(&app);
            }
        })
        .expect("failed to spawn install-queue worker");
}

fn run_task(app: &AppHandle, task: &QueueTask) -> Result<Game, String> {
    let config = Config::load().map_err(|e| e.to_string())?;
    let (paths, db) = open_library(&config).map_err(|e| e.to_string())?;
    let candidate = db
        .get_candidate(task.candidate_id)
        .map_err(|e| e.to_string())?;

    // Progress: the queue runs one task at a time, so whatever new directory
    // appears under <library>/library during this task is its output. Its
    // growth against the candidate's known size is the progress bar.
    let before = dir_names(&paths.games);
    let expected = candidate.size_bytes;
    let stop = Arc::new(AtomicBool::new(false));
    let monitor = {
        let stop = Arc::clone(&stop);
        let games_dir = paths.games.clone();
        let app = app.clone();
        let task_id = task.id;
        std::thread::spawn(move || {
            while !stop.load(Ordering::Relaxed) {
                std::thread::sleep(Duration::from_millis(1200));
                if expected == 0 {
                    continue;
                }
                if let Some(new_dir) = new_entry(&games_dir, &before) {
                    let written = dir_size(&new_dir);
                    let progress = ((written as f64 / expected as f64) as f32).min(0.99);
                    with_task(task_id, |t| t.progress = Some(progress));
                    emit(&app);
                }
            }
        })
    };

    let stage_cb = {
        let app = app.clone();
        let task_id = task.id;
        move |stage: InstallStage| {
            with_task(task_id, |t| t.stage = Some(stage.as_str()));
            emit(&app);
        }
    };
    let opts = InstallOptions {
        adopt_portables: config.adopt_portables,
        progress: Some(&stage_cb),
        default_wine: config.default_wine.as_deref(),
        interactive: config.interactive_installers,
    };

    let result = match &task.archive {
        Some(archive) => {
            install::install_candidate_archive(&db, &paths, candidate.id, Path::new(archive), &opts)
        }
        None => install::install_candidate(&db, &paths, candidate.id, &opts),
    };

    stop.store(true, Ordering::Relaxed);
    let _ = monitor.join();

    let game = result.map_err(|e| e.to_string())?;
    if let Ok(exe) = std::env::current_exe() {
        let exec = format!("\"{}\" --launch {}", exe.display(), game.slug);
        install::finish_install(&paths, &game, &exec);
    }
    Ok(game)
}

fn dir_names(dir: &Path) -> HashSet<OsString> {
    fs::read_dir(dir)
        .map(|entries| {
            entries
                .filter_map(|e| e.ok())
                .map(|e| e.file_name())
                .collect()
        })
        .unwrap_or_default()
}

fn new_entry(dir: &Path, before: &HashSet<OsString>) -> Option<PathBuf> {
    fs::read_dir(dir)
        .ok()?
        .filter_map(|e| e.ok())
        .find(|e| !before.contains(&e.file_name()))
        .map(|e| e.path())
}

fn dir_size(dir: &Path) -> u64 {
    walkdir_size(dir, 0)
}

fn walkdir_size(dir: &Path, depth: usize) -> u64 {
    if depth > 12 {
        return 0;
    }
    let Ok(entries) = fs::read_dir(dir) else {
        return 0;
    };
    entries
        .filter_map(|e| e.ok())
        .map(|e| {
            let Ok(file_type) = e.file_type() else {
                return 0;
            };
            if file_type.is_file() {
                e.metadata().map(|m| m.len()).unwrap_or(0)
            } else if file_type.is_dir() {
                walkdir_size(&e.path(), depth + 1)
            } else {
                0
            }
        })
        .sum()
}
