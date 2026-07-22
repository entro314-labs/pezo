use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use crate::error::{CoreError, Result, io_err};
use crate::model::Game;

/// Where per-game .desktop launchers are written.
fn applications_dir() -> Result<PathBuf> {
    let base = env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| {
            env::var_os("HOME").map(|h| PathBuf::from(h).join(".local").join("share"))
        })
        .ok_or_else(|| CoreError::Config("neither XDG_DATA_HOME nor HOME is set".into()))?;
    Ok(base.join("applications"))
}

fn entry_path(slug: &str) -> Result<PathBuf> {
    Ok(applications_dir()?.join(format!("pezo-{slug}.desktop")))
}

/// Write a launcher entry so the game shows up in the desktop's app grid.
/// `exec` is the full command line that launches the game (already quoted).
pub fn write_desktop_entry(game: &Game, exec: &str, icon: Option<&Path>) -> Result<PathBuf> {
    let dir = applications_dir()?;
    fs::create_dir_all(&dir).map_err(io_err(&dir))?;
    let path = entry_path(&game.slug)?;

    let icon_line = icon
        .map(|p| format!("Icon={}\n", p.display()))
        .unwrap_or_default();
    let content = format!(
        "[Desktop Entry]\n\
         Type=Application\n\
         Name={}\n\
         Comment=Play via Pezo\n\
         Exec={exec}\n\
         {icon_line}\
         Terminal=false\n\
         Categories=Game;\n",
        game.title.replace('\n', " "),
    );
    fs::write(&path, content).map_err(io_err(&path))?;
    Ok(path)
}

/// Remove a game's launcher entry, if it exists.
pub fn remove_desktop_entry(slug: &str) {
    if let Ok(path) = entry_path(slug) {
        let _ = fs::remove_file(path);
    }
}
