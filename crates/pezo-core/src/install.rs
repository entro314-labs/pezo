use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use walkdir::WalkDir;

use crate::classify::{
    Classification, InstallerTech, classify_dir, find_inner_archive, find_setup_exe,
    installer_tech, is_repack, multipart_gaps, multipart_role, steam_common_dir,
};
use crate::config::LibraryPaths;
use crate::covers;
use crate::db::Db;
use crate::deps;
use crate::desktop;
use crate::error::{CoreError, Result, io_err};
use crate::model::{Candidate, CandidateKind, Game, NewGame, Runner};
use crate::naming::slugify;
use crate::wine::{self, WineCommand};

/// Coarse install phases, surfaced to the UI while an install runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstallStage {
    Extracting,
    Copying,
    RunningInstaller,
    InstallingRuntimes,
    Registering,
}

impl InstallStage {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Extracting => "extracting",
            Self::Copying => "copying",
            Self::RunningInstaller => "running-installer",
            Self::InstallingRuntimes => "installing-runtimes",
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
    /// Wine/Proton build that runs Windows installers (None → umu's default).
    pub default_wine: Option<&'a str>,
    /// Show Windows installers' own UI instead of running them unattended.
    pub interactive: bool,
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

/// Post-install niceties, best-effort: cover art and a desktop launcher whose
/// `Exec` line is `exec` (the frontend's own launch command). Returns the
/// launcher's path when one was written.
pub fn finish_install(paths: &LibraryPaths, game: &Game, exec: &str) -> Option<PathBuf> {
    let steam_appid = match game.store.as_deref() {
        Some("gog") => None,
        _ => game.store_id.as_deref(),
    };
    let cover =
        covers::fetch_cover(paths, &game.slug, &game.title, steam_appid).unwrap_or_else(|e| {
            tracing::warn!(slug = %game.slug, "cover fetch failed: {e}");
            None
        });
    match desktop::write_desktop_entry(game, exec, cover.as_deref()) {
        Ok(path) => Some(path),
        Err(e) => {
            tracing::warn!(slug = %game.slug, "desktop entry failed: {e}");
            None
        }
    }
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

    let installer = setup_exe(cand)?;
    // Repacks (and other script-driven setups) ship their payload in a format
    // only their own installer understands: innoextract sees just its temp
    // files. Those run under Wine instead.
    if !inno_has_payload(&innoextract, &installer)? {
        tracing::info!(installer = %installer.display(), "no extractable payload; running installer");
        return install_windows(db, paths, cand, slug, opts);
    }

    // The GOG product id keys umu's per-game protonfixes.
    let gog_id = gog_game_id(&innoextract, &installer);

    opts.stage(InstallStage::Extracting);
    let dest = paths.games.join(slug);
    fs::create_dir_all(&dest).map_err(io_err(&dest))?;

    if let Err(e) = innoextract_into(&innoextract, &installer, &dest) {
        let _ = fs::remove_dir_all(&dest);
        return Err(e);
    }
    // DLC setups shipped alongside unpack into the same folder, as GOG's
    // own installer would put them.
    if let Some(base_id) = &gog_id {
        let probe = paths.games.join(format!(".pezo-probe-{slug}"));
        for dlc in gog_dlc_setups(&innoextract, &installer, base_id, &probe) {
            tracing::info!(dlc = %dlc.display(), "adding DLC");
            if let Err(e) = innoextract_into(&innoextract, &dlc, &dest) {
                // The base game is intact; report the DLC and carry on.
                tracing::warn!(dlc = %dlc.display(), "DLC extraction failed: {e}");
            }
        }
        let _ = fs::remove_dir_all(&probe);
    }

    let game_root = gog_game_root(&dest);

    opts.stage(InstallStage::Registering);
    let store = gog_id.is_some().then_some("gog");
    let game = register_installed_dir(db, paths, cand, &game_root, slug, gog_id, store)?;
    if game.runner == Runner::Proton {
        opts.stage(InstallStage::InstallingRuntimes);
        install_gog_redists(&game, &[&game_root, &dest], opts.default_wine);
    }
    Ok(game)
}

/// Run the runtimes a GOG installer bundles in `__redist` (Visual C++,
/// DirectX, OpenAL, Unreal prerequisites) silently in the game's prefix, as
/// GOG's own setup would. Best effort: the game is installed either way, so
/// failures are logged rather than failing the install. .NET, PhysX and
/// GOG's script interpreter are skipped — Proton/Wine ship their own .NET
/// (wine-mono) and PhysX, and winetricks covers the rest.
fn install_gog_redists(game: &Game, roots: &[&Path], default_wine: Option<&str>) {
    let Some(redist) = roots
        .iter()
        .map(|r| r.join("__redist"))
        .find(|d| d.is_dir())
    else {
        return;
    };
    let mut runtimes: Vec<(PathBuf, &'static [&'static str])> = WalkDir::new(&redist)
        .max_depth(3)
        .follow_links(false)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
        .filter_map(|e| {
            let args = redist_args(e.path().strip_prefix(&redist).ok()?)?;
            Some((e.into_path(), args))
        })
        .collect();
    runtimes.sort();
    if runtimes.is_empty() {
        return;
    }
    let build = match wine::resolve(game.wine_build.as_deref(), default_wine) {
        Ok(build) => build,
        Err(e) => {
            tracing::warn!(game = %game.slug, "skipping bundled runtimes: {e}");
            return;
        }
    };
    for (exe, args) in runtimes {
        let wine_cmd = match crate::launch::game_wine_command(game, build.as_ref()) {
            Ok(cmd) => cmd,
            Err(e) => {
                tracing::warn!(game = %game.slug, "skipping bundled runtimes: {e}");
                return;
            }
        };
        tracing::info!(runtime = %exe.display(), "installing bundled runtime");
        let status = Command::new(&wine_cmd.argv[0])
            .args(&wine_cmd.argv[1..])
            .arg(&exe)
            .args(args)
            .envs(wine_cmd.envs)
            .current_dir(exe.parent().unwrap_or(&redist))
            .status();
        match status {
            // 1638: a newer version is already present; 3010: reboot wanted.
            Ok(s) if s.success() || matches!(s.code(), Some(1638 | 3010)) => {}
            Ok(s) => tracing::warn!(runtime = %exe.display(), "runtime installer exited with {s}"),
            Err(e) => {
                tracing::warn!(runtime = %exe.display(), "runtime installer failed to start: {e}")
            }
        }
    }
}

/// Silent-install switches for a runtime in GOG's `__redist`, by its path
/// relative to that folder. None → not installed (see `install_gog_redists`).
fn redist_args(rel: &Path) -> Option<&'static [&'static str]> {
    let lower = rel.to_string_lossy().to_lowercase().replace('\\', "/");
    let file = lower.rsplit('/').next()?;
    if !file.ends_with(".exe") {
        return None;
    }
    if file == "dxsetup.exe" {
        return Some(&["/silent"]);
    }
    if file == "oalinst.exe" {
        return Some(&["/s"]);
    }
    if file.starts_with("ue4prereqsetup") {
        return Some(&["/quiet", "/norestart"]);
    }
    let dir = lower.split('/').next()?;
    let year = dir.strip_prefix("msvc")?.get(..4)?;
    if !file.starts_with("vc") {
        return None;
    }
    Some(match year {
        "2005" | "2008" => &["/q"],
        "2010" => &["/q", "/norestart"],
        _ => &["/install", "/quiet", "/norestart"],
    })
}

/// The installer exe of a candidate (folder-wrapped installers hold it inside).
fn setup_exe(cand: &Candidate) -> Result<PathBuf> {
    if cand.path.is_dir() {
        find_setup_exe(&cand.path)
            .ok_or_else(|| CoreError::NotFound(format!("setup exe inside {}", cand.path.display())))
    } else {
        Ok(cand.path.clone())
    }
}

/// Whether innoextract can unpack real files (not just setup temp files)
/// from an Inno installer.
fn inno_has_payload(innoextract: &Path, installer: &Path) -> Result<bool> {
    let output = Command::new(innoextract)
        .args(["--list", "--exclude-temp", "--gog"])
        .arg(installer)
        .output()
        .map_err(io_err(innoextract))?;
    if !output.status.success() {
        return Err(CoreError::CommandFailed {
            command: format!("innoextract --list {}", installer.display()),
            status: output.status.to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).trim().to_string(),
        });
    }
    Ok(listing_has_files(&String::from_utf8_lossy(&output.stdout)))
}

/// innoextract `--list` lines look like ` - "path" (size)`; directories end
/// in `/` and carry no size.
fn listing_has_files(listing: &str) -> bool {
    listing
        .lines()
        .any(|line| line.starts_with(" - \"") && !line.trim_end().ends_with("/\""))
}

/// Where an extracted GOG installer's game lives: next to its
/// `goggame-<id>.info`. Modern installers put the game at the top and keep a
/// stub `app/`; older ones put everything under `app/`.
fn gog_game_root(dest: &Path) -> PathBuf {
    let app = dest.join("app");
    if gog_info(dest).is_none()
        && (gog_info(&app).is_some() || find_main_exe(dest).is_none())
        && app.is_dir()
    {
        return app;
    }
    dest.to_path_buf()
}

fn gog_info(dir: &Path) -> Option<PathBuf> {
    fs::read_dir(dir)
        .ok()?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .find(|p| {
            let name = p
                .file_name()
                .map(|n| n.to_string_lossy().to_lowercase())
                .unwrap_or_default();
            name.starts_with("goggame-") && name.ends_with(".info")
        })
}

/// The exe and arguments GOG's own launcher would run: the primary play task
/// in `goggame-<id>.info`. None when there's no info file or its target is
/// missing.
fn gog_play_task(root: &Path) -> Option<(PathBuf, String)> {
    let info: serde_json::Value = serde_json::from_slice(&fs::read(gog_info(root)?).ok()?).ok()?;
    let tasks = info.get("playTasks")?.as_array()?;
    let is_file_task =
        |t: &&serde_json::Value| t.get("type").and_then(|v| v.as_str()) == Some("FileTask");
    let task = tasks
        .iter()
        .filter(is_file_task)
        .find(|t| t.get("isPrimary").and_then(|v| v.as_bool()) == Some(true))
        .or_else(|| tasks.iter().find(is_file_task))?;
    let rel = task.get("path")?.as_str()?.replace('\\', "/");
    let exe = root.join(rel);
    let args = task
        .get("arguments")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    exe.is_file().then_some((exe, args))
}

/// The game's launch target in an installed folder: GOG's play task when
/// present, else the best-guess main exe.
fn main_exe(root: &Path) -> Option<(PathBuf, String)> {
    gog_play_task(root).or_else(|| find_main_exe(root).map(|exe| (exe, String::new())))
}

fn innoextract_into(innoextract: &Path, installer: &Path, dest: &Path) -> Result<()> {
    let output = deprioritized(
        innoextract,
        &[
            "--gog".as_ref(),
            "--exclude-temp".as_ref(),
            "--output-dir".as_ref(),
            dest.as_os_str(),
            installer.as_os_str(),
        ],
    )
    .output()
    .map_err(io_err(innoextract))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(CoreError::CommandFailed {
            command: format!("innoextract {}", installer.display()),
            status: output.status.to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).trim().to_string(),
        })
    }
}

/// GOG setups next to the base installer (same folder, or a subfolder such as
/// `DLC/`) whose `goggame-<id>.info` names the base game as `rootGameId`.
/// Other games sharing the folder (episodes, bundles) are left alone.
fn gog_dlc_setups(
    innoextract: &Path,
    installer: &Path,
    base_id: &str,
    probe: &Path,
) -> Vec<PathBuf> {
    let Some(dir) = installer.parent() else {
        return Vec::new();
    };
    WalkDir::new(dir)
        .max_depth(2)
        .follow_links(false)
        .into_iter()
        .filter_map(|e| e.ok())
        .map(|e| e.into_path())
        .filter(|p| *p != installer)
        .filter(|p| {
            let name = p
                .file_name()
                .map(|n| n.to_string_lossy().to_lowercase())
                .unwrap_or_default();
            name.starts_with("setup_") && name.ends_with(".exe")
        })
        .filter(|p| {
            let Some(id) = gog_game_id(innoextract, p) else {
                return false;
            };
            id != base_id
                && gog_root_game_id(innoextract, p, &id, probe).as_deref() == Some(base_id)
        })
        .collect()
}

/// `rootGameId` from a GOG installer's info file, unpacking only that file.
fn gog_root_game_id(innoextract: &Path, setup: &Path, id: &str, probe: &Path) -> Option<String> {
    let info = format!("goggame-{id}.info");
    let _ = fs::remove_dir_all(probe);
    let status = Command::new(innoextract)
        .args(["--silent", "--include", &info, "--output-dir"])
        .arg(probe)
        .arg(setup)
        .status()
        .ok()?;
    if !status.success() {
        return None;
    }
    let path = [probe.join(&info), probe.join("app").join(&info)]
        .into_iter()
        .find(|p| p.is_file())?;
    let json: serde_json::Value = serde_json::from_slice(&fs::read(path).ok()?).ok()?;
    Some(json.get("rootGameId")?.as_str()?.to_string())
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

/// Installers innoextract can't unpack (repacks, non-Inno setups): run them
/// interactively inside a fresh prefix with the default Wine/Proton build,
/// then register what they installed. Inno setups are pointed at
/// `C:\Games\<slug>` so the game is easy to find afterwards.
fn install_windows(
    db: &Db,
    paths: &LibraryPaths,
    cand: &Candidate,
    slug: &str,
    opts: &InstallOptions<'_>,
) -> Result<Game> {
    let installer = setup_exe(cand)?;
    let build = wine::resolve(None, opts.default_wine)?;
    let prefix = paths.prefixes.join(slug);
    if prefix.exists() {
        return Err(CoreError::InvalidState(format!(
            "prefix {} already exists — remove it or the game it belongs to first",
            prefix.display()
        )));
    }
    fs::create_dir_all(&prefix).map_err(io_err(&prefix))?;
    let result = run_installer_in_prefix(db, cand, slug, opts, &installer, build.as_ref(), &prefix);
    if result.is_err() {
        // The prefix exists only for this install; don't leave it behind.
        let _ = fs::remove_dir_all(&prefix);
    }
    result
}

fn run_installer_in_prefix(
    db: &Db,
    cand: &Candidate,
    slug: &str,
    opts: &InstallOptions<'_>,
    installer: &Path,
    build: Option<&wine::WineBuild>,
    prefix: &Path,
) -> Result<Game> {
    // Repacks always show their UI: unattended, FitGirl's setup stalls under
    // Proton, and under Wine its 32-bit unpackers run out of address space
    // and silently roll back — the "Limit installer to 2 GB of RAM" box on
    // its first page, which has no command-line switch, is the fix.
    let repack = is_repack(installer);
    let interactive = opts.interactive || repack;
    let wine_cmd = WineCommand::new(build, prefix, &format!("umu-{slug}"), None)?;
    let mut cmd = Command::new(&wine_cmd.argv[0]);
    cmd.args(&wine_cmd.argv[1..])
        .args(installer_argv(
            installer_tech(installer)?,
            installer,
            slug,
            interactive,
        ))
        .envs(wine_cmd.envs);
    // Installers (FitGirl's especially) expect to start in their own folder.
    if let Some(dir) = installer.parent() {
        cmd.current_dir(dir);
    }

    opts.stage(InstallStage::RunningInstaller);
    let status = cmd.status().map_err(io_err(installer))?;
    if !status.success() {
        return Err(CoreError::CommandFailed {
            command: format!(
                "{} {}",
                Path::new(&wine_cmd.argv[0]).display(),
                installer.display()
            ),
            status: status.to_string(),
            stderr: "installer exited with a failure status (or was cancelled)".to_string(),
        });
    }

    opts.stage(InstallStage::Registering);
    let drive_c = prefix.join("drive_c");
    let install_dir = installed_game_dir(&drive_c, slug).ok_or_else(|| {
        let hint = if repack {
            " — repacks roll back silently when unpacking fails: tick \"Limit installer to \
             2 GB of RAM\" on the installer's first page, and if it still fails try a Wine \
             build instead of Proton (Settings → Wine & Proton)"
        } else {
            ""
        };
        CoreError::NotFound(format!(
            "the installer finished but no game folder with an executable appeared under {}{hint}",
            drive_c.display()
        ))
    })?;
    let (exe, launch_args) = main_exe(&install_dir).ok_or_else(|| no_executable(&install_dir))?;

    db.insert_game(&NewGame {
        slug,
        title: &cand.display_name,
        runner: Runner::Proton,
        install_path: &install_dir,
        launch_path: &exe,
        prefix_path: Some(prefix),
        source_path: Some(&cand.path),
        store_id: None,
        store: None,
        launch_args: &launch_args,
    })
}

/// Installer command line inside Wine. Every framework is pointed at
/// `C:\Games\<slug>`; unattended runs add its silent switches. Unknown
/// installers always show their own UI.
fn installer_argv(
    tech: InstallerTech,
    installer: &Path,
    slug: &str,
    interactive: bool,
) -> Vec<std::ffi::OsString> {
    let dir = format!("C:\\Games\\{slug}");
    let mut argv: Vec<std::ffi::OsString> = Vec::new();
    match tech {
        InstallerTech::Msi => {
            argv.extend(["msiexec".into(), "/i".into(), installer.into()]);
            argv.push(format!("TARGETDIR={dir}").into());
            if !interactive {
                argv.push("/qn".into());
            }
        }
        InstallerTech::Inno => {
            argv.push(installer.into());
            argv.push(format!("/DIR={dir}").into());
            if !interactive {
                for flag in ["/VERYSILENT", "/SUPPRESSMSGBOXES", "/NORESTART", "/SP-"] {
                    argv.push(flag.into());
                }
            }
        }
        InstallerTech::Nsis => {
            argv.push(installer.into());
            if !interactive {
                argv.push("/S".into());
            }
            // NSIS requires /D last and unquoted.
            argv.push(format!("/D={dir}").into());
        }
        InstallerTech::Other => argv.push(installer.into()),
    }
    argv
}

/// Where an installer put the game inside a fresh prefix: the folder we asked
/// for (`C:\Games\<slug>`) when it honoured `/DIR`, else the largest
/// non-Windows folder it created under the usual install locations.
fn installed_game_dir(drive_c: &Path, slug: &str) -> Option<PathBuf> {
    let asked = drive_c.join("Games").join(slug);
    if find_main_exe(&asked).is_some() {
        return Some(asked);
    }
    // Folders a fresh Wine/Proton prefix already has.
    const PREFIX_DIRS: &[&str] = &[
        "windows",
        "users",
        "programdata",
        "program files",
        "program files (x86)",
        "games",
        "gog games",
        "common files",
        "internet explorer",
        "windows media player",
        "windows nt",
        "windowspowershell",
        "microsoft.net",
        "microsoft",
        "msbuild",
        "reference assemblies",
    ];
    [
        "",
        "Games",
        "GOG Games",
        "Program Files",
        "Program Files (x86)",
    ]
    .iter()
    .filter_map(|root| fs::read_dir(drive_c.join(root)).ok())
    .flat_map(|entries| entries.filter_map(|e| e.ok()))
    .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
    .filter(|e| {
        let name = e.file_name().to_string_lossy().to_lowercase();
        !PREFIX_DIRS.contains(&name.as_str())
    })
    .map(|e| e.path())
    .filter(|dir| find_main_exe(dir).is_some())
    .max_by_key(|dir| tree_size(dir))
}

fn tree_size(dir: &Path) -> u64 {
    WalkDir::new(dir)
        .follow_links(false)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
        .filter_map(|e| e.metadata().ok())
        .map(|m| m.len())
        .sum()
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
    let (install_dir, exe, appid, launch_args) = if cand.path.is_dir() {
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
        let (exe, args) = main_exe(&root).ok_or_else(|| no_executable(&root))?;
        (root, exe, appid, args)
    } else {
        // A bare exe always runs in place — its parent folder is not ours to copy.
        let parent = cand
            .path
            .parent()
            .ok_or_else(|| CoreError::InvalidState("exe has no parent directory".into()))?
            .to_path_buf();
        (parent, cand.path.clone(), None, String::new())
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
        launch_args: &launch_args,
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
        launch_args: "",
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
        launch_args: "",
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
            &[
                "x".as_ref(),
                "-y".as_ref(),
                out_flag.as_ref(),
                archive.as_os_str(),
            ],
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
                &[
                    "x".as_ref(),
                    "-o+".as_ref(),
                    archive.as_os_str(),
                    dest.as_os_str(),
                ],
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
pub(crate) fn deprioritized(program: &Path, args: &[&std::ffi::OsStr]) -> Command {
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

pub(crate) fn run_extractor(cmd: &mut Command) -> Result<()> {
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
            let (exe, launch_args) = main_exe(&root).ok_or_else(|| no_executable(&root))?;
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
                launch_args: &launch_args,
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

/// Error for a folder with no launchable exe. Builder packages (UltraTears and
/// similar ports) only produce their game exe after their own `setup.bat`
/// compiles it from the user's dump, so say how to get there.
fn no_executable(root: &Path) -> CoreError {
    let builder = ["setup.bat", "setup.py"]
        .iter()
        .any(|f| root.join(f).is_file());
    let hint = if builder {
        " — this is a builder package: extract it yourself, put your game dump where its \
         README says, run its setup.bat, then add the folder as a scan folder"
    } else {
        ""
    };
    CoreError::NotFound(format!("no executable in {}{hint}", root.display()))
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
    // Redistributables, and the toolchains recompiled PC ports bundle to build
    // the game on first run (zig, GCC, clang, Python) or keep build output in.
    const EXCLUDE_DIRS: &[&str] = &[
        "windows",
        "redist",
        "directx",
        "_commonredist",
        "dosbox",
        "build",
        "deps",
        "recompiler",
        "zig",
    ];

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
    fn picks_silent_switches_for_bundled_runtimes() {
        let args = |p: &str| redist_args(Path::new(p));
        assert_eq!(
            args("MSVC2010/vcredist_x86.exe"),
            Some(&["/q", "/norestart"][..])
        );
        assert_eq!(args("MSVC2005/vcredist_x86.exe"), Some(&["/q"][..]));
        assert_eq!(
            args("MSVC2022_x64/VC_redist.x64.exe"),
            Some(&["/install", "/quiet", "/norestart"][..])
        );
        assert_eq!(args("DirectX/DXSETUP.exe"), Some(&["/silent"][..]));
        assert_eq!(args("openAL/oalinst.exe"), Some(&["/s"][..]));
        assert_eq!(
            args("UE4REDIST/UE4PrereqSetup_x64.exe"),
            Some(&["/quiet", "/norestart"][..])
        );
        assert_eq!(args("ISI/scriptinterpreter.exe"), None);
        assert_eq!(
            args("dotNet472/NDP472-KB4054530-x86-x64-AllOS-ENU.exe"),
            None
        );
        assert_eq!(args("PHYSX/PhysX-9.17.0524-SystemSoftware.exe"), None);
        assert_eq!(args("DirectX/Feb2010_xinput_x86.cab"), None);
    }

    #[test]
    fn installers_get_target_dir_and_silent_switches() {
        let exe = Path::new("/r/setup.exe");
        let s = |v: Vec<std::ffi::OsString>| {
            v.iter()
                .map(|a| a.to_string_lossy().into_owned())
                .collect::<Vec<_>>()
        };
        assert_eq!(
            s(installer_argv(InstallerTech::Inno, exe, "vh", false)),
            [
                "/r/setup.exe",
                "/DIR=C:\\Games\\vh",
                "/VERYSILENT",
                "/SUPPRESSMSGBOXES",
                "/NORESTART",
                "/SP-"
            ]
        );
        assert_eq!(
            s(installer_argv(InstallerTech::Inno, exe, "vh", true)),
            ["/r/setup.exe", "/DIR=C:\\Games\\vh"]
        );
        assert_eq!(
            s(installer_argv(InstallerTech::Nsis, exe, "vh", false)),
            ["/r/setup.exe", "/S", "/D=C:\\Games\\vh"]
        );
        assert_eq!(
            s(installer_argv(
                InstallerTech::Msi,
                Path::new("/r/g.msi"),
                "vh",
                false
            )),
            [
                "msiexec",
                "/i",
                "/r/g.msi",
                "TARGETDIR=C:\\Games\\vh",
                "/qn"
            ]
        );
        assert_eq!(
            s(installer_argv(InstallerTech::Other, exe, "vh", false)),
            ["/r/setup.exe"]
        );
    }

    #[test]
    fn innoextract_listing_detects_real_payload() {
        // FitGirl: innoextract only sees the installer's own temp files.
        let repack = "Listing \"Vampire Hunters\" - setup data version 5.5.0.1 (unicode)\nDone.\n";
        assert!(!listing_has_files(repack));
        let only_dirs = " - \"app/files/dialogs/de/\"\n - \"app/\"\n";
        assert!(!listing_has_files(only_dirs));
        let gog = " - \"app/files/dialogs/de/\"\n - \"Balrum.exe\" (1.2 MiB)\n";
        assert!(listing_has_files(gog));
    }

    const BALRUM_INFO: &str = r#"{
        "gameId": "1769415595",
        "playTasks": [
            {"category": "document", "type": "FileTask", "path": "manual.pdf"},
            {"category": "game", "isPrimary": true, "type": "FileTask",
             "path": "bin\\x64\\Balrum.exe", "arguments": "-nolauncher"}
        ]
    }"#;

    #[test]
    fn gog_root_is_where_the_info_file_is() {
        // Modern layout: game at the top, stub app/ holding leftovers.
        let modern = tempfile::tempdir().expect("tempdir");
        touch(
            &modern.path().join("goggame-1.info"),
            BALRUM_INFO.as_bytes(),
        );
        touch(&modern.path().join("Balrum.exe"), b"MZ");
        touch(&modern.path().join("app/webcache.zip"), b"zip");
        assert_eq!(gog_game_root(modern.path()), modern.path());

        // Older layout: everything under app/.
        let old = tempfile::tempdir().expect("tempdir");
        touch(
            &old.path().join("app/goggame-1.info"),
            BALRUM_INFO.as_bytes(),
        );
        touch(&old.path().join("app/Game.exe"), b"MZ");
        touch(&old.path().join("tmp/setup.dll"), b"x");
        assert_eq!(gog_game_root(old.path()), old.path().join("app"));
    }

    #[test]
    fn gog_play_task_names_the_launch_target() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        touch(
            &root.join("goggame-1769415595.info"),
            BALRUM_INFO.as_bytes(),
        );
        // A bigger exe the heuristic would otherwise pick.
        touch(&root.join("Launcher.exe"), &[0u8; 4096]);
        touch(&root.join("bin/x64/Balrum.exe"), b"MZ");
        assert_eq!(
            main_exe(root),
            Some((root.join("bin/x64/Balrum.exe"), "-nolauncher".to_string()))
        );
        // Info pointing at a missing file falls back to the heuristic.
        fs::remove_file(root.join("bin/x64/Balrum.exe")).expect("rm");
        assert_eq!(
            main_exe(root),
            Some((root.join("Launcher.exe"), String::new()))
        );
    }

    #[test]
    fn finds_what_an_installer_put_in_the_prefix() {
        let dir = tempfile::tempdir().expect("tempdir");
        let c = dir.path();
        // Stock prefix content.
        touch(&c.join("windows/system32/notepad.exe"), &[0u8; 8192]);
        touch(
            &c.join("Program Files (x86)/Internet Explorer/iexplore.exe"),
            b"MZ",
        );
        touch(&c.join("Program Files/Common Files/x/helper.exe"), b"MZ");
        // The repack ignored /DIR and picked its own folder.
        touch(
            &c.join("Games/Vampire Hunters/VampireHunters.exe"),
            &[0u8; 2048],
        );
        touch(&c.join("Games/Vampire Hunters/data.pak"), &[0u8; 4096]);
        assert_eq!(
            installed_game_dir(c, "vampire-hunters"),
            Some(c.join("Games/Vampire Hunters"))
        );
        // The folder we asked for wins when the installer honoured /DIR.
        touch(&c.join("Games/vampire-hunters/Game.exe"), b"MZ");
        assert_eq!(
            installed_game_dir(c, "vampire-hunters"),
            Some(c.join("Games/vampire-hunters"))
        );
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

    /// Sparse file of `len` bytes: real sizes without writing them.
    fn sized(path: &Path, len: u64) {
        fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
        fs::File::create(path)
            .expect("create")
            .set_len(len)
            .expect("set_len");
    }

    /// Exe layouts of static-recompilation PC ports (and one emulator fork),
    /// from real release archives. The game exe sits next to bundled
    /// toolchains (zig, GCC, clang, Python) that are often far larger.
    #[test]
    fn main_exe_skips_bundled_toolchains_in_recomp_ports() {
        // (exe path, size) per package, and the exe that should win.
        type Case<'a> = (&'a [(&'a str, u64)], Option<&'a str>);
        let cases: &[Case] = &[
            (
                &[
                    ("primerecomp.exe", 10_662_400),
                    ("primegen/primegen.exe", 2_122_791),
                    ("zig/zig.exe", 190_064_128),
                ],
                Some("primerecomp.exe"),
            ),
            (
                &[
                    ("savagewind.exe", 12_824_064),
                    ("Savage Wind Configuration.exe", 82_432),
                    ("swgen/swgen.exe", 2_579_478),
                    ("zig/zig.exe", 190_064_128),
                ],
                Some("savagewind.exe"),
            ),
            (
                &[
                    ("Metroid Dread.exe", 10_837_712),
                    ("recompiler/gcc/Library/bin/gcc.exe", 1_931_278),
                    ("recompiler/python/python.exe", 93_184),
                ],
                Some("Metroid Dread.exe"),
            ),
            (
                &[
                    ("gow3.exe", 88_381_440),
                    ("launcher.exe", 1_370_624),
                    ("builds/gow3-base.exe", 88_381_440),
                    ("builds/gow3-tu6.exe", 88_470_528),
                ],
                Some("gow3.exe"),
            ),
            (&[("openhuey.exe", 1_353_728)], Some("openhuey.exe")),
            (&[("eglantine.exe", 44_299_264)], Some("eglantine.exe")),
            // UltraTears as shipped by its author, a builder: the game exe
            // only exists after setup.bat compiles it from the user's dump.
            (
                &[
                    ("build/bc7build.exe", 829_440),
                    ("deps/clang64/bin/clang.exe", 160_768),
                    ("deps/python/python.exe", 106_208),
                    ("deps/shaders/ultrs-shaders.exe", 78_392_882),
                ],
                None,
            ),
        ];
        for (files, expected) in cases {
            let dir = tempfile::tempdir().expect("tempdir");
            for (rel, len) in *files {
                sized(&dir.path().join(rel), *len);
            }
            assert_eq!(
                find_main_exe(dir.path()),
                expected.map(|e| dir.path().join(e)),
                "{files:?}"
            );
        }
    }

    #[test]
    fn missing_exe_in_builder_package_explains_setup() {
        let dir = tempfile::tempdir().expect("tempdir");
        touch(&dir.path().join("setup.bat"), b"@echo off");
        let msg = no_executable(dir.path()).to_string();
        assert!(msg.contains("setup.bat"), "{msg}");
        let plain = tempfile::tempdir().expect("tempdir");
        assert!(
            !no_executable(plain.path())
                .to_string()
                .contains("setup.bat")
        );
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
        touch(
            &root.join("steamapps").join("appmanifest_228400.acf"),
            b"{}",
        );
        assert_eq!(steam_appid(&root), Some("228400".to_string()));
        assert_eq!(steam_appid(dir.path()), None);
    }
}
