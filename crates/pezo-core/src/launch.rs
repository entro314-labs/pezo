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

fn build_command(game: &Game) -> Result<Command> {
    match game.runner {
        Runner::Proton => {
            let umu = deps::require(
                &["umu-run"],
                "umu-launcher",
                "install it to launch Windows games (https://github.com/Open-Wine-Components/umu-launcher)",
            )?;
            let prefix = game.prefix_path.as_ref().ok_or_else(|| {
                CoreError::InvalidState(format!("game `{}` has no wine prefix", game.slug))
            })?;
            fs::create_dir_all(prefix).map_err(io_err(prefix))?;

            let mut cmd = Command::new(umu);
            cmd.arg(&game.launch_path)
                .current_dir(&game.install_path)
                .env("WINEPREFIX", prefix)
                // GAMEID keys umu's protonfixes; umu-<slug> gets generic handling.
                .env("GAMEID", format!("umu-{}", game.slug));
            Ok(cmd)
        }
        Runner::Dosbox => {
            let dosbox = deps::require(
                &["dosbox-staging", "dosbox"],
                "DOSBox",
                "install dosbox-staging to run DOS games",
            )?;
            let mut cmd = Command::new(dosbox);
            cmd.arg("-conf")
                .arg(&game.launch_path)
                .current_dir(&game.install_path);
            Ok(cmd)
        }
        Runner::Scummvm => {
            let scummvm = deps::require(
                &["scummvm"],
                "ScummVM",
                "install scummvm to run adventure-engine classics",
            )?;
            let mut cmd = Command::new(scummvm);
            cmd.arg(format!("--path={}", game.launch_path.display()))
                .arg("--auto-detect");
            Ok(cmd)
        }
    }
}
