use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{LazyLock, Mutex};

use base64::Engine as _;
use serde::Serialize;
use tauri::{AppHandle, Emitter};

use pezo_core::deps::ToolStatus;
use pezo_core::launch::LaunchOutcome;
use pezo_core::scan::{ScanOptions, ScanOutcome};
use pezo_core::wine::{self, InstallProgress, WineBuild, WineRelease, WineSource};
use pezo_core::{
    Candidate, Config, FsInfo, Game, classify, config, covers, install, launch, open_library, scan,
};

use crate::queue;

type CmdResult<T> = Result<T, String>;

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

/// One scan at a time — a second request while one runs is a UI race, not a
/// reason to hammer the disks twice.
static SCAN_RUNNING: AtomicBool = AtomicBool::new(false);

/// One Wine/Proton download at a time — each is hundreds of MB.
static WINE_INSTALL_RUNNING: AtomicBool = AtomicBool::new(false);

/// Games currently running via launch_game, to refuse double launches.
static RUNNING_GAMES: LazyLock<Mutex<HashSet<i64>>> = LazyLock::new(|| Mutex::new(HashSet::new()));

struct FlagGuard(&'static AtomicBool);

impl FlagGuard {
    fn acquire(flag: &'static AtomicBool, busy_msg: &str) -> Result<Self, String> {
        if flag.swap(true, Ordering::SeqCst) {
            return Err(busy_msg.to_string());
        }
        Ok(Self(flag))
    }
}

impl Drop for FlagGuard {
    fn drop(&mut self) {
        self.0.store(false, Ordering::SeqCst);
    }
}

struct SetGuard {
    set: &'static LazyLock<Mutex<HashSet<i64>>>,
    id: i64,
}

impl SetGuard {
    fn acquire(
        set: &'static LazyLock<Mutex<HashSet<i64>>>,
        id: i64,
        busy_msg: &str,
    ) -> Result<Self, String> {
        if !set.lock().unwrap().insert(id) {
            return Err(busy_msg.to_string());
        }
        Ok(Self { set, id })
    }
}

impl Drop for SetGuard {
    fn drop(&mut self) {
        self.set.lock().unwrap().remove(&self.id);
    }
}

#[derive(Clone, Serialize)]
struct ScanProgress {
    done: usize,
    total: usize,
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
pub fn set_adopt_portables(enabled: bool) -> CmdResult<Config> {
    let mut config = Config::load().map_err(err)?;
    config.adopt_portables = enabled;
    config.save().map_err(err)?;
    Ok(config)
}

#[tauri::command]
pub fn set_interactive_installers(enabled: bool) -> CmdResult<Config> {
    let mut config = Config::load().map_err(err)?;
    config.interactive_installers = enabled;
    config.save().map_err(err)?;
    Ok(config)
}

#[tauri::command]
pub fn doctor() -> Vec<ToolStatus> {
    pezo_core::deps::doctor()
}

/// Window-button layout from GNOME settings, so client-side decorations
/// mirror the desktop instead of imposing a foreign arrangement.
#[derive(Clone, Serialize)]
pub struct DesktopLayout {
    pub left: Vec<String>,
    pub right: Vec<String>,
}

#[tauri::command]
pub fn desktop_layout() -> DesktopLayout {
    let fallback = DesktopLayout {
        left: Vec::new(),
        right: vec!["minimize".into(), "maximize".into(), "close".into()],
    };
    let Ok(out) = std::process::Command::new("gsettings")
        .args(["get", "org.gnome.desktop.wm.preferences", "button-layout"])
        .output()
    else {
        return fallback;
    };
    if !out.status.success() {
        return fallback;
    }
    let raw = String::from_utf8_lossy(&out.stdout);
    let raw = raw.trim().trim_matches('\'');
    let (left_raw, right_raw) = raw.split_once(':').unwrap_or(("", raw));
    let parse = |part: &str| -> Vec<String> {
        part.split(',')
            .map(str::trim)
            .filter(|b| matches!(*b, "minimize" | "maximize" | "close"))
            .map(String::from)
            .collect()
    };
    let layout = DesktopLayout {
        left: parse(left_raw),
        right: parse(right_raw),
    };
    if layout.left.is_empty() && layout.right.is_empty() {
        fallback
    } else {
        layout
    }
}

/// (tool name as reported by doctor) → package per package manager.
const TOOL_PACKAGES: &[(&str, &str, &str, &str)] = &[
    // (tool, pacman, apt, dnf) — empty string = not packaged there.
    ("umu-launcher", "umu-launcher", "", "umu-launcher"),
    ("innoextract", "innoextract", "innoextract", "innoextract"),
    (
        "bsdtar (libarchive)",
        "libarchive",
        "libarchive-tools",
        "bsdtar",
    ),
    ("7-Zip", "7zip", "7zip", "p7zip"),
    ("unrar", "unrar", "unrar", ""),
    (
        "DOSBox",
        "dosbox-staging",
        "dosbox-staging",
        "dosbox-staging",
    ),
    ("ScummVM", "scummvm", "scummvm", "scummvm"),
    ("GameMode", "gamemode", "gamemode", "gamemode"),
    ("MangoHud", "mangohud", "mangohud", "mangohud"),
    ("winetricks", "winetricks", "winetricks", "winetricks"),
];

fn package_manager() -> Option<&'static str> {
    ["pacman", "apt-get", "dnf"]
        .into_iter()
        .find(|pm| pezo_core::deps::which(pm).is_some())
}

fn package_for(tool: &str, pm: &str) -> Option<&'static str> {
    let entry = TOOL_PACKAGES.iter().find(|(name, ..)| *name == tool)?;
    let pkg = match pm {
        "pacman" => entry.1,
        "apt-get" => entry.2,
        "dnf" => entry.3,
        _ => "",
    };
    (!pkg.is_empty()).then_some(pkg)
}

/// Tools the app can install itself on this distro (used to show buttons).
#[tauri::command]
pub fn installable_tools() -> Vec<String> {
    let Some(pm) = package_manager() else {
        return Vec::new();
    };
    if pezo_core::deps::which("pkexec").is_none() {
        return Vec::new();
    }
    TOOL_PACKAGES
        .iter()
        .filter(|(name, ..)| package_for(name, pm).is_some())
        .map(|(name, ..)| name.to_string())
        .collect()
}

/// Install a missing tool through the distro's package manager, authorized
/// via polkit (pkexec) so the desktop shows its own password prompt.
#[tauri::command]
pub async fn install_tool(tool: String) -> CmdResult<()> {
    run_blocking(move || {
        let pm = package_manager().ok_or_else(|| {
            pezo_core::CoreError::Unsupported(
                "no supported package manager found (pacman, apt, dnf)".into(),
            )
        })?;
        let pkg = package_for(&tool, pm).ok_or_else(|| {
            pezo_core::CoreError::Unsupported(format!(
                "`{tool}` is not packaged for {pm} — install it manually"
            ))
        })?;
        let pkexec = pezo_core::deps::require(
            &["pkexec"],
            "pkexec",
            "install polkit so Pezo can request admin rights for package installs",
        )?;
        let args: Vec<&str> = match pm {
            "pacman" => vec!["pacman", "-S", "--noconfirm", "--needed", pkg],
            "apt-get" => vec!["apt-get", "install", "-y", pkg],
            _ => vec!["dnf", "install", "-y", pkg],
        };
        let output = std::process::Command::new(&pkexec)
            .args(&args)
            .output()
            .map_err(|e| pezo_core::CoreError::Unsupported(format!("failed to run pkexec: {e}")))?;
        if output.status.success() {
            Ok(())
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            Err(pezo_core::CoreError::CommandFailed {
                command: format!("pkexec {}", args.join(" ")),
                status: output.status.to_string(),
                stderr: stderr
                    .lines()
                    .last()
                    .unwrap_or("cancelled")
                    .trim()
                    .to_string(),
            })
        }
    })
    .await
}

/// Filesystem the library root sits on — the UI warns when Proton will
/// misbehave there (NTFS/FAT).
#[tauri::command]
pub fn library_fs_info() -> Option<FsInfo> {
    let root = Config::load().ok()?.library_root?;
    config::fs_info(&root)
}

#[tauri::command]
pub async fn scan_installers(app: AppHandle, full: bool) -> CmdResult<ScanOutcome> {
    let guard = FlagGuard::acquire(&SCAN_RUNNING, "a scan is already running")?;
    let result = run_blocking(move || {
        let config = Config::load()?;
        let (paths, db) = open_library(&config)?;
        let emit = |done: usize, total: usize| {
            // Small scans chatter needlessly; report every few entries.
            if total == 0 || done == total || done.is_multiple_of(4) {
                let _ = app.emit("scan-progress", ScanProgress { done, total });
            }
        };
        let opts = ScanOptions {
            full,
            progress: Some(&emit),
        };
        scan::scan_library(&db, &paths, &config.scan_dirs, &opts)
    })
    .await;
    drop(guard);
    result
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

/// Archives inside a folder candidate, for the disc-set picker.
#[tauri::command]
pub fn list_candidate_archives(candidate_id: i64) -> CmdResult<Vec<String>> {
    let config = Config::load().map_err(err)?;
    let (_, db) = open_library(&config).map_err(err)?;
    let candidate = db.get_candidate(candidate_id).map_err(err)?;
    if !candidate.path.is_dir() {
        return Ok(Vec::new());
    }
    Ok(classify::list_dir_archives(&candidate.path)
        .into_iter()
        .map(|p| p.to_string_lossy().to_string())
        .collect())
}

/// Enqueue an install; the queue worker runs them one at a time.
#[tauri::command]
pub fn queue_install(app: AppHandle, candidate_id: i64, archive: Option<String>) -> CmdResult<u64> {
    let config = Config::load().map_err(err)?;
    let (_, db) = open_library(&config).map_err(err)?;
    let candidate = db.get_candidate(candidate_id).map_err(err)?;
    if let Some(game_id) = candidate.installed_game_id {
        return Err(format!("already installed as game {game_id}"));
    }
    Ok(queue::enqueue(
        &app,
        candidate_id,
        candidate.display_name,
        archive,
    ))
}

#[tauri::command]
pub fn queue_state() -> Vec<queue::QueueTask> {
    queue::snapshot()
}

#[tauri::command]
pub fn queue_cancel(app: AppHandle, task_id: u64) -> bool {
    queue::cancel(&app, task_id)
}

#[tauri::command]
pub fn queue_clear_finished(app: AppHandle) {
    queue::clear_finished(&app);
}

/// A game's cached cover as a data URL, if one was fetched.
#[tauri::command]
pub fn get_cover(game_id: i64) -> CmdResult<Option<String>> {
    let config = Config::load().map_err(err)?;
    let (paths, db) = open_library(&config).map_err(err)?;
    let game = db.get_game(game_id).map_err(err)?;
    let Some(path) = covers::cover_path(&paths, &game.slug) else {
        return Ok(None);
    };
    let bytes = std::fs::read(&path).map_err(err)?;
    let encoded = base64::engine::general_purpose::STANDARD.encode(bytes);
    Ok(Some(format!("data:image/jpeg;base64,{encoded}")))
}

/// Try to fetch covers for every game that doesn't have one. Returns how many
/// were fetched.
#[tauri::command]
pub async fn fetch_missing_covers() -> CmdResult<usize> {
    run_blocking(|| {
        let config = Config::load()?;
        let (paths, db) = open_library(&config)?;
        let mut fetched = 0;
        for game in db.list_games()? {
            if covers::cover_path(&paths, &game.slug).is_some() {
                continue;
            }
            let steam_appid = match game.store.as_deref() {
                Some("gog") => None,
                _ => game.store_id.as_deref(),
            };
            if covers::fetch_cover(&paths, &game.slug, &game.title, steam_appid)
                .ok()
                .flatten()
                .is_some()
            {
                fetched += 1;
            }
        }
        Ok(fetched)
    })
    .await
}

/// Save per-game launch options. `wine_build` None follows the default build.
#[tauri::command]
pub fn update_launch_options(
    game_id: i64,
    launch_args: String,
    env_vars: String,
    wrappers: String,
    wine_build: Option<String>,
) -> CmdResult<Game> {
    let config = Config::load().map_err(err)?;
    let (_, db) = open_library(&config).map_err(err)?;
    // Only a newly picked build must exist here; a pick carried over from
    // another machine shouldn't block saving the other options.
    if let Some(name) = &wine_build
        && db.get_game(game_id).map_err(err)?.wine_build.as_ref() != Some(name)
    {
        wine::find(name).map_err(err)?;
    }
    db.update_launch_options(
        game_id,
        &launch_args,
        &env_vars,
        &wrappers,
        wine_build.as_deref(),
    )
    .map_err(err)?;
    db.get_game(game_id).map_err(err)
}

/// Open winetricks in the game's prefix (detached).
#[tauri::command]
pub fn run_winetricks(game_id: i64) -> CmdResult<()> {
    let config = Config::load().map_err(err)?;
    let (_, db) = open_library(&config).map_err(err)?;
    launch::run_winetricks(&db, game_id, config.default_wine.as_deref()).map_err(err)
}

/// Wine and Proton builds installed on this machine.
#[tauri::command]
pub fn list_wine_builds() -> CmdResult<Vec<WineBuild>> {
    wine::detect().map_err(err)
}

/// Projects Pezo can download builds from.
#[tauri::command]
pub fn wine_sources() -> &'static [WineSource] {
    wine::SOURCES
}

#[tauri::command]
pub async fn list_wine_releases(source: String) -> CmdResult<Vec<WineRelease>> {
    run_blocking(move || wine::list_releases(&source)).await
}

#[derive(Clone, Serialize)]
struct WineInstallEvent {
    source: String,
    tag: Option<String>,
    progress: InstallProgress,
}

/// Download and unpack a build (`tag` None → latest). Progress streams as
/// `wine-install-progress` events.
#[tauri::command]
pub async fn install_wine_build(
    app: AppHandle,
    source: String,
    tag: Option<String>,
) -> CmdResult<WineBuild> {
    let guard = FlagGuard::acquire(
        &WINE_INSTALL_RUNNING,
        "a Wine/Proton download is already running",
    )?;
    let result = run_blocking(move || {
        let report = |progress: InstallProgress| {
            let _ = app.emit(
                "wine-install-progress",
                WineInstallEvent {
                    source: source.clone(),
                    tag: tag.clone(),
                    progress,
                },
            );
        };
        wine::install_release(&source, tag.as_deref(), &report)
    })
    .await;
    drop(guard);
    result
}

#[tauri::command]
pub fn remove_wine_build(name: String) -> CmdResult<Config> {
    let mut config = Config::load().map_err(err)?;
    wine::remove(&mut config, &name).map_err(err)?;
    Ok(config)
}

/// Set the build games use unless they pick their own (None → umu's default).
#[tauri::command]
pub fn set_default_wine(name: Option<String>) -> CmdResult<Config> {
    let mut config = Config::load().map_err(err)?;
    wine::set_default(&mut config, name.as_deref()).map_err(err)?;
    Ok(config)
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
    let guard = SetGuard::acquire(&RUNNING_GAMES, game_id, "this game is already running")?;
    std::thread::spawn(move || {
        let _guard = guard;
        let result = (|| {
            let config = Config::load()?;
            let (_, db) = open_library(&config)?;
            launch::launch_game(&db, game_id, config.default_wine.as_deref())
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

/// Remove a game from the library; optionally delete its installed files and
/// prefix (originals outside the library are never touched).
#[tauri::command]
pub fn remove_game(game_id: i64, delete_files: bool) -> CmdResult<()> {
    let config = Config::load().map_err(err)?;
    let (paths, db) = open_library(&config).map_err(err)?;
    install::remove_game(&db, &paths, game_id, delete_files).map_err(err)
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
