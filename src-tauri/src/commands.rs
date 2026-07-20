use std::path::PathBuf;

use serde::Serialize;
use tauri::{AppHandle, Emitter};

use pezo_core::deps::ToolStatus;
use pezo_core::launch::LaunchOutcome;
use pezo_core::scan::ScanOutcome;
use pezo_core::{Candidate, Config, Game, install, launch, open_library, scan};

type CmdResult<T> = Result<T, String>;

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

#[tauri::command]
pub fn get_config() -> CmdResult<Config> {
    Config::load().map_err(err)
}

#[tauri::command]
pub fn set_library_root(path: String) -> CmdResult<Config> {
    let mut config = Config::load().map_err(err)?;
    config.library_root = Some(PathBuf::from(path));
    config
        .library()
        .map_err(err)?
        .ensure_layout()
        .map_err(err)?;
    config.save().map_err(err)?;
    Ok(config)
}

#[tauri::command]
pub fn add_scan_dir(path: String) -> CmdResult<Config> {
    let dir = PathBuf::from(path);
    if !dir.is_dir() {
        return Err(format!("{} is not a directory", dir.display()));
    }
    let mut config = Config::load().map_err(err)?;
    if config.library_root.as_deref() != Some(dir.as_path()) && !config.scan_dirs.contains(&dir) {
        config.scan_dirs.push(dir);
        config.save().map_err(err)?;
    }
    Ok(config)
}

#[tauri::command]
pub fn remove_scan_dir(path: String) -> CmdResult<Config> {
    let dir = PathBuf::from(path);
    let mut config = Config::load().map_err(err)?;
    config.scan_dirs.retain(|d| *d != dir);
    config.save().map_err(err)?;
    Ok(config)
}

#[tauri::command]
pub fn doctor() -> Vec<ToolStatus> {
    pezo_core::deps::doctor()
}

#[tauri::command]
pub async fn scan_installers() -> CmdResult<ScanOutcome> {
    run_blocking(|| {
        let config = Config::load()?;
        let (paths, db) = open_library(&config)?;
        scan::scan_library(&db, &paths, &config.scan_dirs)
    })
    .await
}

#[tauri::command]
pub fn list_candidates() -> CmdResult<Vec<Candidate>> {
    let config = Config::load().map_err(err)?;
    let (_, db) = open_library(&config).map_err(err)?;
    db.list_candidates().map_err(err)
}

#[tauri::command]
pub fn list_games() -> CmdResult<Vec<Game>> {
    let config = Config::load().map_err(err)?;
    let (_, db) = open_library(&config).map_err(err)?;
    db.list_games().map_err(err)
}

#[tauri::command]
pub async fn install_candidate(candidate_id: i64) -> CmdResult<Game> {
    run_blocking(move || {
        let config = Config::load()?;
        let (paths, db) = open_library(&config)?;
        install::install_candidate(&db, &paths, candidate_id)
    })
    .await
}

#[derive(Clone, Serialize)]
struct GameExited {
    outcome: Option<LaunchOutcome>,
    error: Option<String>,
    game_id: i64,
}

/// Launch a game without blocking the UI; a `game-exited` event fires when it
/// quits (or fails to start), carrying playtime or the error.
#[tauri::command]
pub fn launch_game(app: AppHandle, game_id: i64) -> CmdResult<()> {
    std::thread::spawn(move || {
        let result = (|| {
            let config = Config::load()?;
            let (_, db) = open_library(&config)?;
            launch::launch_game(&db, game_id)
        })();
        let payload = match result {
            Ok(outcome) => GameExited {
                outcome: Some(outcome),
                error: None,
                game_id,
            },
            Err(e) => GameExited {
                outcome: None,
                error: Some(e.to_string()),
                game_id,
            },
        };
        if let Err(emit_err) = app.emit("game-exited", payload) {
            eprintln!("failed to emit game-exited event: {emit_err}");
        }
    });
    Ok(())
}

/// Remove a game from the library (does not delete files on disk).
#[tauri::command]
pub fn remove_game(game_id: i64) -> CmdResult<()> {
    let config = Config::load().map_err(err)?;
    let (_, db) = open_library(&config).map_err(err)?;
    db.remove_game(game_id).map_err(err)
}

async fn run_blocking<T, F>(f: F) -> CmdResult<T>
where
    T: Send + 'static,
    F: FnOnce() -> pezo_core::Result<T> + Send + 'static,
{
    tauri::async_runtime::spawn_blocking(f)
        .await
        .map_err(err)?
        .map_err(err)
}
