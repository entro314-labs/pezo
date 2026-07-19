use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use walkdir::WalkDir;

use crate::classify::{Classification, classify_dir, find_setup_exe};
use crate::config::LibraryPaths;
use crate::db::Db;
use crate::deps;
use crate::error::{CoreError, Result, io_err};
use crate::model::{Candidate, CandidateKind, Game, Runner};
use crate::naming::slugify;

/// Install a scanned candidate and register the resulting game.
pub fn install_candidate(db: &Db, paths: &LibraryPaths, candidate_id: i64) -> Result<Game> {
    let candidate = db.get_candidate(candidate_id)?;
    if let Some(game_id) = candidate.installed_game_id {
        return Err(CoreError::InvalidState(format!(
            "candidate {candidate_id} is already installed as game {game_id}"
        )));
    }
    paths.ensure_layout()?;

    let slug = unique_slug(db, &slugify(&candidate.display_name))?;
    let game = match candidate.kind {
        CandidateKind::GogInstaller | CandidateKind::InnoInstaller => {
            install_inno(db, paths, &candidate, &slug)?
        }
        CandidateKind::WindowsInstaller => install_windows(db, paths, &candidate, &slug)?,
        CandidateKind::PortableWindows => register_portable(db, paths, &candidate, &slug)?,
        CandidateKind::DosGame => register_dos(db, &candidate, &candidate.path, &slug)?,
        CandidateKind::ScummVmGame => register_scummvm(db, &candidate, &candidate.path, &slug)?,
        CandidateKind::Archive => install_archive(db, paths, &candidate, &slug)?,
        CandidateKind::Unknown => {
            return Err(CoreError::Unsupported(format!(
                "no install strategy for {} — no executable, DOSBox or ScummVM markers found",
                candidate.path.display()
            )));
        }
    };

    db.link_candidate(candidate.id, game.id)?;
    tracing::info!(game = %game.title, runner = game.runner.as_str(), "installed");
    Ok(game)
}

/// GOG / Inno Setup installers: unpack with innoextract, no wine involved.
fn install_inno(db: &Db, paths: &LibraryPaths, cand: &Candidate, slug: &str) -> Result<Game> {
    let innoextract = deps::require(
        &["innoextract"],
        "innoextract",
        "install it via your package manager (e.g. pacman -S innoextract)",
    )?;

    // Folder-wrapped installers: locate the setup exe inside.
    let installer: PathBuf = if cand.path.is_dir() {
        find_setup_exe(&cand.path).ok_or_else(|| {
            CoreError::NotFound(format!("setup exe inside {}", cand.path.display()))
        })?
    } else {
        cand.path.clone()
    };

    let dest = paths.games.join(slug);
    fs::create_dir_all(&dest).map_err(io_err(&dest))?;

    let output = Command::new(&innoextract)
        .arg("--gog")
        .arg("--exclude-temp")
        .arg("--dir")
        .arg(&dest)
        .arg(&installer)
        .output()
        .map_err(io_err(&innoextract))?;
    if !output.status.success() {
        let _ = fs::remove_dir_all(&dest);
        return Err(CoreError::CommandFailed {
            command: format!("innoextract {}", installer.display()),
            status: output.status.to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).trim().to_string(),
        });
    }

    // innoextract usually creates an `app/` dir; treat it as the game root if present.
    let game_root = if dest.join("app").is_dir() {
        dest.join("app")
    } else {
        dest
    };

    register_installed_dir(db, paths, cand, &game_root, slug)
}

/// Non-Inno Windows installers: run them interactively inside a fresh prefix via umu.
fn install_windows(db: &Db, paths: &LibraryPaths, cand: &Candidate, slug: &str) -> Result<Game> {
    let umu = deps::require(
        &["umu-run"],
        "umu-launcher",
        "install it to run Windows installers (https://github.com/Open-Wine-Components/umu-launcher)",
    )?;

    let prefix = paths.prefixes.join(slug);
    fs::create_dir_all(&prefix).map_err(io_err(&prefix))?;

    let status = Command::new(&umu)
        .arg(&cand.path)
        .env("WINEPREFIX", &prefix)
        .env("GAMEID", format!("umu-{slug}"))
        .status()
        .map_err(io_err(&umu))?;
    if !status.success() {
        return Err(CoreError::CommandFailed {
            command: format!("umu-run {}", cand.path.display()),
            status: status.to_string(),
            stderr: "installer exited with a failure status".to_string(),
        });
    }

    // The game landed somewhere in the prefix; find its main exe.
    let drive_c = prefix.join("drive_c");
    let exe = find_main_exe(&drive_c).ok_or_else(|| {
        CoreError::NotFound(format!(
            "no game executable found under {} after the installer finished",
            drive_c.display()
        ))
    })?;
    let install_dir = exe.parent().unwrap_or(&drive_c).to_path_buf();

    db.insert_game(
        slug,
        &cand.display_name,
        Runner::Proton,
        &install_dir,
        &exe,
        Some(&prefix),
        Some(&cand.path),
    )
}

/// Portable folders / bare exes: register in place, no copying.
fn register_portable(db: &Db, paths: &LibraryPaths, cand: &Candidate, slug: &str) -> Result<Game> {
    let (install_dir, exe) = if cand.path.is_dir() {
        let exe = find_main_exe(&cand.path).ok_or_else(|| {
            CoreError::NotFound(format!("no executable in {}", cand.path.display()))
        })?;
        (cand.path.clone(), exe)
    } else {
        let parent = cand
            .path
            .parent()
            .ok_or_else(|| CoreError::InvalidState("exe has no parent directory".into()))?
            .to_path_buf();
        (parent, cand.path.clone())
    };

    let prefix = paths.prefixes.join(slug);
    db.insert_game(
        slug,
        &cand.display_name,
        Runner::Proton,
        &install_dir,
        &exe,
        Some(&prefix),
        Some(&cand.path),
    )
}

fn register_dos(db: &Db, cand: &Candidate, dir: &Path, slug: &str) -> Result<Game> {
    let conf = find_dosbox_conf(dir).ok_or_else(|| {
        CoreError::NotFound(format!(
            "no dosbox .conf file in {} — create one and re-install",
            dir.display()
        ))
    })?;
    db.insert_game(
        slug,
        &cand.display_name,
        Runner::Dosbox,
        dir,
        &conf,
        None,
        Some(&cand.path),
    )
}

fn register_scummvm(db: &Db, cand: &Candidate, dir: &Path, slug: &str) -> Result<Game> {
    db.insert_game(
        slug,
        &cand.display_name,
        Runner::Scummvm,
        dir,
        dir,
        None,
        Some(&cand.path),
    )
}

/// Archives: extract with bsdtar, then handle whatever came out.
fn install_archive(db: &Db, paths: &LibraryPaths, cand: &Candidate, slug: &str) -> Result<Game> {
    let bsdtar = deps::require(
        &["bsdtar"],
        "bsdtar",
        "install libarchive tools to extract archives (e.g. apt install libarchive-tools)",
    )?;

    let dest = paths.games.join(slug);
    fs::create_dir_all(&dest).map_err(io_err(&dest))?;

    let output = Command::new(&bsdtar)
        .arg("-xf")
        .arg(&cand.path)
        .arg("-C")
        .arg(&dest)
        .output()
        .map_err(io_err(&bsdtar))?;
    if !output.status.success() {
        let _ = fs::remove_dir_all(&dest);
        return Err(CoreError::CommandFailed {
            command: format!("bsdtar -xf {}", cand.path.display()),
            status: output.status.to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).trim().to_string(),
        });
    }

    // If the archive held a single wrapper folder, descend into it.
    let game_root = single_child_dir(&dest).unwrap_or(dest);
    register_installed_dir(db, paths, cand, &game_root, slug)
}

/// Register an already-on-disk game directory, choosing the runner from its contents.
fn register_installed_dir(
    db: &Db,
    paths: &LibraryPaths,
    cand: &Candidate,
    dir: &Path,
    slug: &str,
) -> Result<Game> {
    match classify_dir(dir)? {
        Classification::Candidate(CandidateKind::DosGame) => register_dos(db, cand, dir, slug),
        Classification::Candidate(CandidateKind::ScummVmGame) => {
            register_scummvm(db, cand, dir, slug)
        }
        Classification::Candidate(
            CandidateKind::PortableWindows
            | CandidateKind::GogInstaller
            | CandidateKind::InnoInstaller
            | CandidateKind::WindowsInstaller,
        ) => {
            let exe = find_main_exe(dir).ok_or_else(|| {
                CoreError::NotFound(format!("no executable in {}", dir.display()))
            })?;
            let prefix = paths.prefixes.join(slug);
            db.insert_game(
                slug,
                &cand.display_name,
                Runner::Proton,
                dir,
                &exe,
                Some(&prefix),
                Some(&cand.path),
            )
        }
        Classification::Candidate(CandidateKind::Archive | CandidateKind::Unknown)
        | Classification::Ignore => Err(CoreError::Unsupported(format!(
            "extracted {} but found nothing launchable inside",
            dir.display()
        ))),
    }
}

/// Heuristic: the "main" exe is a shallow, large exe that isn't a helper/redist.
pub fn find_main_exe(dir: &Path) -> Option<PathBuf> {
    const EXCLUDE: &[&str] = &[
        "unins",
        "vcredist",
        "vc_redist",
        "dxsetup",
        "dxwebsetup",
        "directx",
        "dotnet",
        "redist",
        "crashreport",
        "crashhandler",
        "launcher_settings",
        "unitycrashhandler",
    ];
    const EXCLUDE_DIRS: &[&str] = &["windows", "redist", "directx", "_commonredist", "dosbox"];

    WalkDir::new(dir)
        .max_depth(4)
        .follow_links(false)
        .into_iter()
        .filter_entry(|e| {
            let name = e.file_name().to_string_lossy().to_lowercase();
            !(e.file_type().is_dir() && EXCLUDE_DIRS.contains(&name.as_str()))
        })
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
        .filter(|e| {
            let name = e.file_name().to_string_lossy().to_lowercase();
            name.ends_with(".exe") && !EXCLUDE.iter().any(|x| name.contains(x))
        })
        .max_by_key(|e| {
            let size = e.metadata().map(|m| m.len()).unwrap_or(0);
            let shallow_bonus: u64 = match e.depth() {
                0 | 1 => 1 << 40,
                2 => 1 << 39,
                _ => 0,
            };
            size + shallow_bonus
        })
        .map(|e| e.into_path())
}

fn find_dosbox_conf(dir: &Path) -> Option<PathBuf> {
    let mut confs: Vec<PathBuf> = WalkDir::new(dir)
        .max_depth(3)
        .follow_links(false)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
        .filter(|e| {
            let name = e.file_name().to_string_lossy().to_lowercase();
            name.ends_with(".conf") && name.contains("dosbox")
        })
        .map(|e| e.into_path())
        .collect();
    // GOG ships `dosbox<Game>.conf` + `dosbox<Game>_single.conf`; prefer the
    // `_single` variant, which launches the game directly.
    confs.sort_by_key(|p| {
        let name = p
            .file_name()
            .map(|n| n.to_string_lossy().to_lowercase())
            .unwrap_or_default();
        (!name.contains("single"), name)
    });
    confs.into_iter().next()
}

fn single_child_dir(dir: &Path) -> Option<PathBuf> {
    let entries: Vec<_> = fs::read_dir(dir)
        .ok()?
        .filter_map(|e| e.ok())
        .filter(|e| !e.file_name().to_string_lossy().starts_with('.'))
        .collect();
    match entries.as_slice() {
        [only] if only.path().is_dir() => Some(only.path()),
        _ => None,
    }
}

fn unique_slug(db: &Db, base: &str) -> Result<String> {
    if !db.slug_taken(base)? {
        return Ok(base.to_string());
    }
    for n in 2..1000 {
        let candidate = format!("{base}-{n}");
        if !db.slug_taken(&candidate)? {
            return Ok(candidate);
        }
    }
    Err(CoreError::InvalidState(format!(
        "could not find a free slug for `{base}`"
    )))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn touch(path: &Path, contents: &[u8]) {
        fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
        fs::write(path, contents).expect("write");
    }

    #[test]
    fn main_exe_prefers_large_shallow_and_skips_helpers() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        touch(&root.join("unins000.exe"), &[0u8; 5000]);
        touch(&root.join("Game.exe"), &[0u8; 100]);
        touch(&root.join("redist").join("vcredist_x64.exe"), &[0u8; 90000]);
        assert_eq!(find_main_exe(root), Some(root.join("Game.exe")));
    }

    #[test]
    fn dosbox_conf_prefers_single_variant() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        touch(&root.join("dosboxTyrian.conf"), b"[sdl]\n");
        touch(&root.join("dosboxTyrian_single.conf"), b"[sdl]\n");
        assert_eq!(
            find_dosbox_conf(root),
            Some(root.join("dosboxTyrian_single.conf"))
        );
    }
}
