use std::ffi::OsString;
use std::fs;
use std::process::Command;
use std::time::Instant;

use serde::{Deserialize, Serialize};

use crate::db::Db;
use crate::deps;
use crate::error::{CoreError, Result, io_err};
use crate::model::{Game, Runner};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LaunchOutcome {
    pub game_id: i64,
    pub exit_code: Option<i32>,
    pub seconds: i64,
}

/// Launch a game, block until it exits, and record playtime.
pub fn launch_game(db: &Db, game_id: i64) -> Result<LaunchOutcome> {
    let game = db.get_game(game_id)?;
    let mut cmd = build_command(&game)?;

    tracing::info!(game = %game.title, runner = game.runner.as_str(), "launching");
    let started = Instant::now();
    let status = cmd.status().map_err(io_err(&game.launch_path))?;
    let seconds = started.elapsed().as_secs() as i64;

    db.record_play(game.id, seconds)?;
    Ok(LaunchOutcome {
        game_id: game.id,
        exit_code: status.code(),
        seconds,
    })
}

/// Open winetricks inside a Proton game's prefix (via umu, detached).
pub fn run_winetricks(db: &Db, game_id: i64) -> Result<()> {
    let game = db.get_game(game_id)?;
    if game.runner != Runner::Proton {
        return Err(CoreError::Unsupported(format!(
            "`{}` runs under {}, which has no wine prefix",
            game.title,
            game.runner.as_str()
        )));
    }
    let umu = require_umu()?;
    let prefix = game.prefix_path.as_ref().ok_or_else(|| {
        CoreError::InvalidState(format!("game `{}` has no wine prefix", game.slug))
    })?;
    fs::create_dir_all(prefix).map_err(io_err(prefix))?;

    let mut cmd = Command::new(umu);
    cmd.arg("winetricks")
        .arg("--gui")
        .env("WINEPREFIX", prefix)
        .env("GAMEID", game_umu_id(&game));
    apply_store(&mut cmd, &game);
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }
    cmd.spawn().map_err(io_err(&game.launch_path))?;
    Ok(())
}

fn require_umu() -> Result<std::path::PathBuf> {
    deps::require(
        &["umu-run"],
        "umu-launcher",
        "install it to launch Windows games (https://github.com/Open-Wine-Components/umu-launcher)",
    )
}

/// GAMEID for umu: a real store id unlocks per-game protonfixes; otherwise a
/// generic slug id.
fn game_umu_id(game: &Game) -> String {
    match &game.store_id {
        Some(id) => format!("umu-{id}"),
        None => format!("umu-{}", game.slug),
    }
}

fn apply_store(cmd: &mut Command, game: &Game) {
    if let Some(store) = &game.store {
        cmd.env("STORE", store);
    }
}

fn build_command(game: &Game) -> Result<Command> {
    // Base program + args per runner.
    let mut argv: Vec<OsString> = Vec::new();
    let mut envs: Vec<(&'static str, OsString)> = Vec::new();

    match game.runner {
        Runner::Proton => {
            let umu = require_umu()?;
            let prefix = game.prefix_path.as_ref().ok_or_else(|| {
                CoreError::InvalidState(format!("game `{}` has no wine prefix", game.slug))
            })?;
            fs::create_dir_all(prefix).map_err(io_err(prefix))?;
            argv.push(umu.into_os_string());
            argv.push(game.launch_path.clone().into_os_string());
            envs.push(("WINEPREFIX", prefix.clone().into_os_string()));
            envs.push(("GAMEID", game_umu_id(game).into()));
            if let Some(store) = &game.store {
                envs.push(("STORE", store.into()));
            }
        }
        Runner::Dosbox => {
            let dosbox = deps::require(
                &["dosbox-staging", "dosbox"],
                "DOSBox",
                "install dosbox-staging to run DOS games",
            )?;
            argv.push(dosbox.into_os_string());
            argv.push("-conf".into());
            argv.push(game.launch_path.clone().into_os_string());
        }
        Runner::Scummvm => {
            let scummvm = deps::require(
                &["scummvm"],
                "ScummVM",
                "install scummvm to run adventure-engine classics",
            )?;
            argv.push(scummvm.into_os_string());
            argv.push(format!("--path={}", game.launch_path.display()).into());
            argv.push("--auto-detect".into());
        }
    }

    // Per-game extra arguments.
    for arg in game.launch_args.split_whitespace() {
        argv.push(arg.into());
    }

    // Wrappers prepend to the command line: `gamemoderun mangohud umu-run …`.
    let mut wrapped: Vec<OsString> = Vec::new();
    for wrapper in game.wrappers.split(',').map(str::trim).filter(|w| !w.is_empty()) {
        let (bin, name, hint) = match wrapper {
            "gamemode" => (
                "gamemoderun",
                "GameMode",
                "install gamemode (Arch: pacman -S gamemode) or disable it for this game",
            ),
            "mangohud" => (
                "mangohud",
                "MangoHud",
                "install mangohud (Arch: pacman -S mangohud) or disable it for this game",
            ),
            other => {
                return Err(CoreError::Unsupported(format!(
                    "unknown launch wrapper `{other}`"
                )));
            }
        };
        let path = deps::require(&[bin], name, hint)?;
        wrapped.push(path.into_os_string());
    }
    wrapped.extend(argv);

    let mut cmd = Command::new(&wrapped[0]);
    cmd.args(&wrapped[1..]);
    // Give the game its own process group so signals aimed at the app (or a
    // crashing app) never take the game down with it.
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }
    match game.runner {
        // ScummVM takes the game dir as an argument, not a cwd.
        Runner::Scummvm => {}
        _ => {
            cmd.current_dir(&game.install_path);
        }
    }
    for (key, value) in envs {
        cmd.env(key, value);
    }
    // Per-game environment lines (KEY=VALUE).
    for line in game.env_vars.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some((key, value)) = line.split_once('=') {
            cmd.env(key.trim(), value.trim());
        }
    }
    Ok(cmd)
}
