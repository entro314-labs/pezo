use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use walkdir::WalkDir;

use crate::classify::{
    Classification, classify_dir, find_inner_archive, find_setup_exe, multipart_gaps,
    multipart_role, steam_common_dir,
};
use crate::config::LibraryPaths;
use crate::covers;
use crate::db::Db;
use crate::deps;
use crate::desktop;
use crate::error::{CoreError, Result, io_err};
use crate::model::{Candidate, CandidateKind, Game, NewGame, Runner};
use crate::naming::slugify;

/// Coarse install phases, surfaced to the UI while an install runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstallStage {
    Extracting,
    Copying,
    RunningInstaller,
    Registering,
}

impl InstallStage {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Extracting => "extracting",
            Self::Copying => "copying",
            Self::RunningInstaller => "running-installer",
            Self::Registering => "registering",
        }
    }
}

/// Knobs for an install run.
#[derive(Default)]
pub struct InstallOptions<'a> {
    /// Copy portable games into the library instead of registering in place.
    pub adopt_portables: bool,
    /// Called as the install moves between phases.
    pub progress: Option<&'a dyn Fn(InstallStage)>,
}

impl InstallOptions<'_> {
    fn stage(&self, stage: InstallStage) {
        if let Some(cb) = self.progress {
            cb(stage);
        }
    }
}

/// Install a scanned candidate and register the resulting game.
pub fn install_candidate(
    db: &Db,
    paths: &LibraryPaths,
    candidate_id: i64,
    opts: &InstallOptions<'_>,
) -> Result<Game> {
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
            install_inno(db, paths, &candidate, &slug, opts)?
        }
        CandidateKind::WindowsInstaller => install_windows(db, paths, &candidate, &slug, opts)?,
        CandidateKind::PortableWindows => register_portable(db, paths, &candidate, &slug, opts)?,
        CandidateKind::DosGame => register_dos(db, &candidate, &candidate.path, &slug)?,
        CandidateKind::ScummVmGame => register_scummvm(db, &candidate, &candidate.path, &slug)?,
        CandidateKind::Archive => {
            let archive = resolve_archive(&candidate)?;
            install_archive(db, paths, &candidate, &archive, &slug, opts)?
        }
        CandidateKind::Unknown => {
            return Err(CoreError::Unsupported(format!(
                "no install strategy for {} — no executable, installer, archive, DOSBox or \
                 ScummVM markers found. Multi-game collections and console disc images \
                 aren't installable as a single game",
                candidate.path.display()
            )));
        }
    };

    db.link_candidate(candidate.id, game.id)?;
    tracing::info!(game = %game.title, runner = game.runner.as_str(), "installed");
    Ok(game)
}

/// Install a specific archive found inside a folder candidate (disc sets and
/// other multi-archive folders where classification refused to guess).
pub fn install_candidate_archive(
    db: &Db,
    paths: &LibraryPaths,
    candidate_id: i64,
    archive: &Path,
    opts: &InstallOptions<'_>,
) -> Result<Game> {
    let candidate = db.get_candidate(candidate_id)?;
    if let Some(game_id) = candidate.installed_game_id {
        return Err(CoreError::InvalidState(format!(
            "candidate {candidate_id} is already installed as game {game_id}"
        )));
    }
    if !archive.starts_with(&candidate.path) || !archive.is_file() {
        return Err(CoreError::InvalidState(format!(
            "{} is not an archive inside {}",
            archive.display(),
            candidate.path.display()
        )));
    }
    paths.ensure_layout()?;

    let slug = unique_slug(db, &slugify(&candidate.display_name))?;
    let game = install_archive(db, paths, &candidate, archive, &slug, opts)?;
    db.link_candidate(candidate.id, game.id)?;
    Ok(game)
}

/// Remove a game from the library. With `delete_files`, its installed copy
/// and wine prefix are deleted too — but only when they live inside the
/// library's own directories; adopted originals elsewhere are never touched.
pub fn remove_game(db: &Db, paths: &LibraryPaths, game_id: i64, delete_files: bool) -> Result<()> {
    let game = db.get_game(game_id)?;
    if delete_files {
        if game.install_path.starts_with(&paths.games) && game.install_path.is_dir() {
            fs::remove_dir_all(&game.install_path).map_err(io_err(&game.install_path))?;
        }
        if let Some(prefix) = &game.prefix_path
            && prefix.starts_with(&paths.prefixes)
            && prefix.is_dir()
        {
            fs::remove_dir_all(prefix).map_err(io_err(prefix))?;
        }
    }
    covers::remove_cover(paths, &game.slug);
    desktop::remove_desktop_entry(&game.slug);
    db.remove_game(game_id)
}

/// GOG / Inno Setup installers: unpack with innoextract, no wine involved.
fn install_inno(
    db: &Db,
    paths: &LibraryPaths,
    cand: &Candidate,
    slug: &str,
    opts: &InstallOptions<'_>,
) -> Result<Game> {
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

    // The GOG product id keys umu's per-game protonfixes.
    let gog_id = gog_game_id(&innoextract, &installer);

    opts.stage(InstallStage::Extracting);
    let dest = paths.games.join(slug);
    fs::create_dir_all(&dest).map_err(io_err(&dest))?;

    let output = deprioritized(
        &innoextract,
        &[
            "--gog".as_ref(),
            "--exclude-temp".as_ref(),
            "--dir".as_ref(),
            dest.as_os_str(),
            installer.as_os_str(),
        ],
    )
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

    opts.stage(InstallStage::Registering);
    let store = gog_id.is_some().then_some("gog");
    register_installed_dir(db, paths, cand, &game_root, slug, gog_id, store)
}

/// Ask innoextract for the GOG product id embedded in an installer.
fn gog_game_id(innoextract: &Path, installer: &Path) -> Option<String> {
    let output = Command::new(innoextract)
        .arg("--gog-game-id")
        .arg("--silent")
        .arg(installer)
        .output()
        .ok()?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let id: String = stdout
        .chars()
        .skip_while(|c| !c.is_ascii_digit())
        .take_while(|c| c.is_ascii_digit())
        .collect();
    (!id.is_empty()).then_some(id)
}

/// Steam appid from a rip's `steamapps/appmanifest_<id>.acf`, if present.
fn steam_appid(dir: &Path) -> Option<String> {
    let steamapps = dir.join("steamapps");
    for entry in fs::read_dir(steamapps).ok()?.filter_map(|e| e.ok()) {
        let name = entry.file_name().to_string_lossy().to_string();
        if let Some(id) = name
            .strip_prefix("appmanifest_")
            .and_then(|r| r.strip_suffix(".acf"))
            && id.chars().all(|c| c.is_ascii_digit())
        {
            return Some(id.to_string());
        }
    }
    None
}

/// Non-Inno Windows installers: run them interactively inside a fresh prefix via umu.
fn install_windows(
    db: &Db,
    paths: &LibraryPaths,
    cand: &Candidate,
    slug: &str,
    opts: &InstallOptions<'_>,
) -> Result<Game> {
    let umu = deps::require(
        &["umu-run"],
        "umu-launcher",
        "install it to run Windows installers (https://github.com/Open-Wine-Components/umu-launcher)",
    )?;

    let prefix = paths.prefixes.join(slug);
    fs::create_dir_all(&prefix).map_err(io_err(&prefix))?;

    opts.stage(InstallStage::RunningInstaller);
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

    opts.stage(InstallStage::Registering);
    // The game landed somewhere in the prefix; find its main exe.
    let drive_c = prefix.join("drive_c");
    let exe = find_main_exe(&drive_c).ok_or_else(|| {
        CoreError::NotFound(format!(
            "no game executable found under {} after the installer finished",
            drive_c.display()
        ))
    })?;
    let install_dir = exe.parent().unwrap_or(&drive_c).to_path_buf();

    db.insert_game(&NewGame {
        slug,
        title: &cand.display_name,
        runner: Runner::Proton,
        install_path: &install_dir,
        launch_path: &exe,
        prefix_path: Some(&prefix),
        source_path: Some(&cand.path),
        store_id: None,
        store: None,
    })
}

/// Portable folders / bare exes. Folders found outside the library are copied
/// in ("adopted") when enabled, so they survive their source disk vanishing;
/// otherwise they register in place.
fn register_portable(
    db: &Db,
    paths: &LibraryPaths,
    cand: &Candidate,
    slug: &str,
    opts: &InstallOptions<'_>,
) -> Result<Game> {
    let (install_dir, exe, appid) = if cand.path.is_dir() {
        // Steam library rips: the game proper lives in steamapps/common/<Game>.
        let appid = steam_appid(&cand.path);
        let source_root = steam_common_dir(&cand.path)
            .map(|common| single_child_dir(&common).unwrap_or(common))
            .unwrap_or_else(|| cand.path.clone());

        let root = if opts.adopt_portables && !source_root.starts_with(&paths.root) {
            opts.stage(InstallStage::Copying);
            let dest = paths.games.join(slug);
            if let Err(e) = copy_dir_recursive(&source_root, &dest) {
                let _ = fs::remove_dir_all(&dest);
                return Err(e);
            }
            dest
        } else {
            source_root
        };

        opts.stage(InstallStage::Registering);
        let exe = find_main_exe(&root).ok_or_else(|| {
            CoreError::NotFound(format!("no executable in {}", root.display()))
        })?;
        (root, exe, appid)
    } else {
        // A bare exe always runs in place — its parent folder is not ours to copy.
        let parent = cand
            .path
            .parent()
            .ok_or_else(|| CoreError::InvalidState("exe has no parent directory".into()))?
            .to_path_buf();
        (parent, cand.path.clone(), None)
    };

    let prefix = paths.prefixes.join(slug);
    db.insert_game(&NewGame {
        slug,
        title: &cand.display_name,
        runner: Runner::Proton,
        install_path: &install_dir,
        launch_path: &exe,
        prefix_path: Some(&prefix),
        source_path: Some(&cand.path),
        store_id: appid,
        store: None,
    })
}

fn register_dos(db: &Db, cand: &Candidate, dir: &Path, slug: &str) -> Result<Game> {
    let conf = find_dosbox_conf(dir).ok_or_else(|| {
        CoreError::NotFound(format!(
            "no dosbox .conf file in {} — create one and re-install",
            dir.display()
        ))
    })?;
    db.insert_game(&NewGame {
        slug,
        title: &cand.display_name,
        runner: Runner::Dosbox,
        install_path: dir,
        launch_path: &conf,
        prefix_path: None,
        source_path: Some(&cand.path),
        store_id: None,
        store: None,
    })
}

fn register_scummvm(db: &Db, cand: &Candidate, dir: &Path, slug: &str) -> Result<Game> {
    db.insert_game(&NewGame {
        slug,
        title: &cand.display_name,
        runner: Runner::Scummvm,
        install_path: dir,
        launch_path: dir,
        prefix_path: None,
        source_path: Some(&cand.path),
        store_id: None,
        store: None,
    })
}

/// The archive file a candidate refers to: itself, or the one meaningful
/// archive inside a wrapper folder.
fn resolve_archive(cand: &Candidate) -> Result<PathBuf> {
    if cand.path.is_dir() {
        find_inner_archive(&cand.path).ok_or_else(|| {
            CoreError::NotFound(format!(
                "no extractable archive inside {}",
                cand.path.display()
            ))
        })
    } else {
        Ok(cand.path.clone())
    }
}

/// Archives: extract into the library, then handle whatever came out.
/// A failure at any stage removes the partial extraction.
fn install_archive(
    db: &Db,
    paths: &LibraryPaths,
    cand: &Candidate,
    archive: &Path,
    slug: &str,
    opts: &InstallOptions<'_>,
) -> Result<Game> {
    // Refuse up front if a split archive has holes — extraction would only
    // fail halfway through with a cryptic tool error.
    let gaps = multipart_gaps(archive);
    if !gaps.is_empty() {
        let list = gaps
            .iter()
            .map(|n| n.to_string())
            .collect::<Vec<_>>()
            .join(", ");
        return Err(CoreError::InvalidState(format!(
            "{} is a split archive missing part(s) {list} — finish the download and rescan",
            archive.display()
        )));
    }

    opts.stage(InstallStage::Extracting);
    let dest = paths.games.join(slug);
    fs::create_dir_all(&dest).map_err(io_err(&dest))?;

    let result = extract_archive(archive, &dest).and_then(|_| {
        opts.stage(InstallStage::Registering);
        // If the archive held a single wrapper folder, descend into it.
        let game_root = single_child_dir(&dest).unwrap_or_else(|| dest.clone());
        register_installed_dir(db, paths, cand, &game_root, slug, None, None)
    });
    if result.is_err() {
        let _ = fs::remove_dir_all(&dest);
    }
    result
}

/// Extract `archive` into `dest` with the best available tool. bsdtar covers
/// single-file archives; split sets and some RARs need 7-Zip or unrar, so on
/// failure the next tool is tried against a cleaned destination.
fn extract_archive(archive: &Path, dest: &Path) -> Result<()> {
    let name = archive
        .file_name()
        .map(|n| n.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    let is_split = multipart_role(&name).is_some();
    let is_rar = name.ends_with(".rar");
    let mut failures: Vec<String> = Vec::new();

    // bsdtar cannot read split archives at all — don't waste a pass.
    if !is_split {
        if let Some(bsdtar) = deps::which("bsdtar") {
            let mut cmd = deprioritized(
                &bsdtar,
                &[
                    "-xf".as_ref(),
                    archive.as_os_str(),
                    "-C".as_ref(),
                    dest.as_os_str(),
                ],
            );
            match run_extractor(&mut cmd) {
                Ok(()) => return Ok(()),
                Err(e) => {
                    failures.push(format!("bsdtar: {e}"));
                    reset_dir(dest)?;
                }
            }
        } else {
            failures.push("bsdtar: not installed (libarchive)".into());
        }
    }

    // 7-Zip reads split .7z/.zip/.rar sets when pointed at the first part.
    if let Some(sevenzip) = deps::which_any(&["7z", "7zz", "7za"]) {
        let out_flag = format!("-o{}", dest.display());
        let mut cmd = deprioritized(
            &sevenzip,
            &["x".as_ref(), "-y".as_ref(), out_flag.as_ref(), archive.as_os_str()],
        );
        match run_extractor(&mut cmd) {
            Ok(()) => return Ok(()),
            Err(e) => {
                failures.push(format!("7z: {e}"));
                reset_dir(dest)?;
            }
        }
    } else {
        failures.push("7z: not installed (Arch: pacman -S 7zip · Debian: apt install 7zip)".into());
    }

    if is_rar {
        if let Some(unrar) = deps::which("unrar") {
            let mut cmd = deprioritized(
                &unrar,
                &["x".as_ref(), "-o+".as_ref(), archive.as_os_str(), dest.as_os_str()],
            );
            match run_extractor(&mut cmd) {
                Ok(()) => return Ok(()),
                Err(e) => {
                    failures.push(format!("unrar: {e}"));
                    reset_dir(dest)?;
                }
            }
        } else {
            failures.push(
                "unrar: not installed (Arch: pacman -S unrar · Debian: apt install unrar)".into(),
            );
        }
    }

    Err(CoreError::Unsupported(format!(
        "could not extract {}: {}",
        archive.display(),
        failures.join("; ")
    )))
}

/// Wrap a heavy command with idle IO priority and lowered niceness so hour-long
/// extractions don't make the desktop stutter. Falls back to the bare command
/// when ionice/nice aren't available.
fn deprioritized(program: &Path, args: &[&std::ffi::OsStr]) -> Command {
    if let Some(ionice) = deps::which("ionice") {
        let mut cmd = Command::new(ionice);
        cmd.args(["-c", "3"]);
        if deps::which("nice").is_some() {
            cmd.arg("nice").arg("-n").arg("10");
        }
        cmd.arg(program).args(args);
        cmd
    } else {
        let mut cmd = Command::new(program);
        cmd.args(args);
        cmd
    }
}

fn run_extractor(cmd: &mut Command) -> Result<()> {
    let program = cmd.get_program().to_string_lossy().to_string();
    let output = cmd
        .output()
        .map_err(|e| CoreError::Unsupported(format!("failed to run {program}: {e}")))?;
    if output.status.success() {
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let detail = stderr.lines().last().unwrap_or("").trim().to_string();
        Err(CoreError::CommandFailed {
            command: program,
            status: output.status.to_string(),
            stderr: detail,
        })
    }
}

/// Clear a directory between extraction attempts so a half-written try can't
/// contaminate the next one (or the final game root).
fn reset_dir(dir: &Path) -> Result<()> {
    fs::remove_dir_all(dir).map_err(io_err(dir))?;
    fs::create_dir_all(dir).map_err(io_err(dir))
}

/// Register an already-on-disk game directory, choosing the runner from its contents.
fn register_installed_dir(
    db: &Db,
    paths: &LibraryPaths,
    cand: &Candidate,
    dir: &Path,
    slug: &str,
    store_id: Option<String>,
    store: Option<&str>,
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
            let root = steam_common_dir(dir)
                .map(|common| single_child_dir(&common).unwrap_or(common))
                .unwrap_or_else(|| dir.to_path_buf());
            let store_id = store_id.or_else(|| steam_appid(dir));
            let exe = find_main_exe(&root).ok_or_else(|| {
                CoreError::NotFound(format!("no executable in {}", root.display()))
            })?;
            let prefix = paths.prefixes.join(slug);
            db.insert_game(&NewGame {
                slug,
                title: &cand.display_name,
                runner: Runner::Proton,
                install_path: &root,
                launch_path: &exe,
                prefix_path: Some(&prefix),
                source_path: Some(&cand.path),
                store_id,
                store,
            })
        }
        Classification::Candidate(CandidateKind::Archive | CandidateKind::Unknown)
        | Classification::Ignore => Err(CoreError::Unsupported(format!(
            "{} contains no launchable game — it may be a disc image or a nested \
             archive that needs manual handling",
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

/// Copy a directory tree (no symlink following; symlinks are skipped).
fn copy_dir_recursive(src: &Path, dest: &Path) -> Result<()> {
    fs::create_dir_all(dest).map_err(io_err(dest))?;
    for entry in WalkDir::new(src).follow_links(false).min_depth(1) {
        let entry = entry.map_err(|e| CoreError::Unsupported(format!("copy walk failed: {e}")))?;
        let rel = entry
            .path()
            .strip_prefix(src)
            .map_err(|e| CoreError::InvalidState(format!("copy path mismatch: {e}")))?;
        let target = dest.join(rel);
        let file_type = entry.file_type();
        if file_type.is_dir() {
            fs::create_dir_all(&target).map_err(io_err(&target))?;
        } else if file_type.is_file() {
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent).map_err(io_err(parent))?;
            }
            fs::copy(entry.path(), &target).map_err(io_err(entry.path()))?;
        }
    }
    Ok(())
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

    #[test]
    fn copies_directory_trees() {
        let dir = tempfile::tempdir().expect("tempdir");
        let src = dir.path().join("src");
        touch(&src.join("a.txt"), b"a");
        touch(&src.join("sub").join("b.txt"), b"b");
        let dest = dir.path().join("dest");
        copy_dir_recursive(&src, &dest).expect("copy");
        assert!(dest.join("a.txt").is_file());
        assert!(dest.join("sub").join("b.txt").is_file());
    }

    #[test]
    fn steam_appid_from_manifest() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path().join("rip");
        touch(&root.join("steamapps").join("appmanifest_228400.acf"), b"{}");
        assert_eq!(steam_appid(&root), Some("228400".to_string()));
        assert_eq!(steam_appid(dir.path()), None);
    }
}
