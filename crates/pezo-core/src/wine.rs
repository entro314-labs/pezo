//! Wine and Proton builds: find the ones already on this machine, download
//! new ones from their GitHub releases, and turn a build choice into the
//! command that runs a Windows program inside a prefix.
//!
//! A build is identified by its directory name (`GE-Proton11-7`,
//! `wine-11.19-amd64-wow64`, …). Games and the config store that name, not a
//! path, so a library moved to another machine resolves against whatever that
//! machine has installed.
//!
//! Proton builds run through umu-run with `PROTONPATH` pointing at them. Wine
//! builds run their own `bin/wine` directly. umu prefixes are plain Wine
//! prefixes (umu symlinks `pfx` to the prefix itself), so a game can switch
//! between the two kinds without losing its prefix.

use std::cmp::Ordering;
use std::ffi::OsString;
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256, Sha512};

use crate::config::{Config, data_home, home_dir};
use crate::deps;
use crate::error::{CoreError, Result, io_err};
use crate::install::{deprioritized, run_extractor};

/// Name of the system Wine (the `wine` on PATH).
pub const SYSTEM_WINE: &str = "system";

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum WineKind {
    /// Proton build, launched through umu-run.
    Proton,
    /// Plain Wine build, launched through its own `bin/wine`.
    Wine,
}

impl WineKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Proton => "proton",
            Self::Wine => "wine",
        }
    }
}

/// Where a detected build came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum BuildSource {
    /// A `compatibilitytools.d` folder (ProtonUp-Qt, umu, distro packages, Pezo).
    CompatTools,
    /// Valve's Proton installed by Steam into `steamapps/common`.
    Steam,
    /// umu's own download folder.
    Umu,
    /// A Lutris Wine runner.
    Lutris,
    /// Downloaded by Pezo into its data folder.
    Pezo,
    /// The `wine` on PATH.
    System,
}

/// An installed Wine or Proton build.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WineBuild {
    /// Directory name; the id games and the config refer to.
    pub name: String,
    pub kind: WineKind,
    /// Build root: holds `proton` (Proton) or `bin/wine` (Wine).
    pub path: PathBuf,
    pub source: BuildSource,
    /// Whether Pezo may delete it (user-writable download folders only —
    /// never Steam's, a distro package's or Lutris's own).
    pub removable: bool,
}

impl WineBuild {
    fn wine_binary(&self) -> PathBuf {
        self.path.join("bin").join("wine")
    }
}

struct Location {
    dir: PathBuf,
    kind: WineKind,
    source: BuildSource,
    removable: bool,
}

/// Where Pezo installs Proton builds: the same `compatibilitytools.d` umu
/// downloads into (`$XDG_DATA_HOME/Steam`), so nothing is downloaded twice
/// and Steam sees them too.
fn proton_install_dir(data_home: &Path) -> PathBuf {
    data_home.join("Steam").join("compatibilitytools.d")
}

/// Where Pezo installs Wine builds.
fn wine_install_dir(data_home: &Path) -> PathBuf {
    data_home.join("pezo").join("wine")
}

/// Every folder searched for builds, in priority order: when two folders
/// hold a build with the same name, the first one wins.
fn locations(home: &Path, data_home: &Path) -> Vec<Location> {
    let proton = |dir: PathBuf, source, removable| Location {
        dir,
        kind: WineKind::Proton,
        source,
        removable,
    };
    let wine = |dir: PathBuf, source, removable| Location {
        dir,
        kind: WineKind::Wine,
        source,
        removable,
    };
    vec![
        proton(
            proton_install_dir(data_home),
            BuildSource::CompatTools,
            true,
        ),
        // Steam's own folder, when XDG_DATA_HOME points somewhere else.
        proton(
            home.join(".local/share/Steam/compatibilitytools.d"),
            BuildSource::CompatTools,
            true,
        ),
        proton(
            home.join(".steam/root/compatibilitytools.d"),
            BuildSource::CompatTools,
            true,
        ),
        proton(
            home.join(".var/app/com.valvesoftware.Steam/data/Steam/compatibilitytools.d"),
            BuildSource::CompatTools,
            true,
        ),
        proton(
            data_home.join("umu/compatibilitytools"),
            BuildSource::Umu,
            true,
        ),
        proton(
            PathBuf::from("/usr/share/steam/compatibilitytools.d"),
            BuildSource::CompatTools,
            false,
        ),
        proton(
            PathBuf::from("/usr/local/share/steam/compatibilitytools.d"),
            BuildSource::CompatTools,
            false,
        ),
        proton(
            home.join(".local/share/Steam/steamapps/common"),
            BuildSource::Steam,
            false,
        ),
        wine(wine_install_dir(data_home), BuildSource::Pezo, true),
        wine(
            data_home.join("lutris/runners/wine"),
            BuildSource::Lutris,
            false,
        ),
    ]
}

fn is_build(dir: &Path, kind: WineKind) -> bool {
    match kind {
        WineKind::Proton => dir.join("proton").is_file(),
        WineKind::Wine => dir.join("bin").join("wine").is_file(),
    }
}

fn detect_in(locations: &[Location], system_wine: Option<&Path>) -> Vec<WineBuild> {
    let mut builds: Vec<WineBuild> = Vec::new();
    let mut seen_paths: Vec<PathBuf> = Vec::new();
    for loc in locations {
        let Ok(entries) = fs::read_dir(&loc.dir) else {
            continue;
        };
        for entry in entries.filter_map(|e| e.ok()) {
            let path = entry.path();
            if !is_build(&path, loc.kind) {
                continue;
            }
            let name = entry.file_name().to_string_lossy().into_owned();
            // `~/.steam/root` is usually a symlink to `~/.local/share/Steam`.
            let canonical = path.canonicalize().unwrap_or_else(|_| path.clone());
            if seen_paths.contains(&canonical) || builds.iter().any(|b| b.name == name) {
                continue;
            }
            seen_paths.push(canonical);
            builds.push(WineBuild {
                name,
                kind: loc.kind,
                path,
                source: loc.source,
                removable: loc.removable,
            });
        }
    }
    // /usr/bin/wine → /usr, so `bin/wine` resolves back to it.
    if let Some(root) = system_wine.and_then(Path::parent).and_then(Path::parent) {
        builds.push(WineBuild {
            name: SYSTEM_WINE.to_string(),
            kind: WineKind::Wine,
            path: root.to_path_buf(),
            source: BuildSource::System,
            removable: false,
        });
    }
    // Proton first, newest first within a kind.
    builds.sort_by(|a, b| {
        a.kind
            .cmp(&b.kind)
            .then_with(|| natural_cmp(&b.name, &a.name))
    });
    builds
}

/// Every Wine and Proton build installed on this machine.
pub fn detect() -> Result<Vec<WineBuild>> {
    let locations = locations(&home_dir()?, &data_home()?);
    Ok(detect_in(&locations, deps::which("wine").as_deref()))
}

/// Look up an installed build by name.
pub fn find(name: &str) -> Result<WineBuild> {
    detect()?
        .into_iter()
        .find(|b| b.name == name)
        .ok_or_else(|| {
            CoreError::NotFound(format!(
                "Wine/Proton build `{name}` is not installed on this machine — install it \
                 (Settings → Wine & Proton, or `pezo wine install`) or pick another build"
            ))
        })
}

/// The build a Windows game runs with: its own pick, else the configured
/// default. None means umu-run chooses (latest UMU-Proton).
pub fn resolve(game_choice: Option<&str>, default: Option<&str>) -> Result<Option<WineBuild>> {
    game_choice.or(default).map(find).transpose()
}

/// Make `name` the default build for games without their own pick (None
/// restores umu's default).
pub fn set_default(config: &mut Config, name: Option<&str>) -> Result<()> {
    if let Some(name) = name {
        find(name)?;
    }
    config.default_wine = name.map(str::to_owned);
    config.save()
}

/// Delete a build Pezo is allowed to remove. Clears the default if it pointed
/// at this build; games that picked it fail to launch with an install hint
/// until they're switched to another build.
pub fn remove(config: &mut Config, name: &str) -> Result<()> {
    let build = find(name)?;
    if !build.removable {
        return Err(CoreError::Unsupported(format!(
            "`{name}` is managed elsewhere ({}) — remove it there",
            build.path.display()
        )));
    }
    fs::remove_dir_all(&build.path).map_err(io_err(&build.path))?;
    if config.default_wine.as_deref() == Some(name) {
        config.default_wine = None;
        config.save()?;
    }
    tracing::info!(build = name, "removed wine build");
    Ok(())
}

/// Program and environment that run a Windows executable (appended by the
/// caller) inside `prefix` with `build`.
pub struct WineCommand {
    pub argv: Vec<OsString>,
    pub envs: Vec<(&'static str, OsString)>,
}

impl WineCommand {
    /// Proton (or no pick) → `umu-run` with `PROTONPATH`; Wine → its `bin/wine`.
    /// `game_id` and `store` key umu's per-game protonfixes.
    pub fn new(
        build: Option<&WineBuild>,
        prefix: &Path,
        game_id: &str,
        store: Option<&str>,
    ) -> Result<Self> {
        let mut envs: Vec<(&'static str, OsString)> =
            vec![("WINEPREFIX", prefix.as_os_str().to_owned())];
        let program = match build {
            Some(b) if b.kind == WineKind::Wine => {
                // Proton does this itself: keep Wine from adding the game's
                // (and its installer's) Start-menu entries to the desktop.
                envs.push(("WINEDLLOVERRIDES", "winemenubuilder.exe=d".into()));
                b.wine_binary()
            }
            _ => {
                let umu = deps::require(
                    &["umu-run"],
                    "umu-launcher",
                    "install it to run Windows programs with Proton \
                     (https://github.com/Open-Wine-Components/umu-launcher)",
                )?;
                envs.push(("GAMEID", game_id.into()));
                if let Some(store) = store {
                    envs.push(("STORE", store.into()));
                }
                if let Some(b) = build {
                    envs.push(("PROTONPATH", b.path.as_os_str().to_owned()));
                }
                umu
            }
        };
        Ok(Self {
            argv: vec![program.into_os_string()],
            envs,
        })
    }

    /// The command for `winetricks --gui` in this prefix: umu's built-in
    /// winetricks verb for Proton, the winetricks script pointed at the build's
    /// wine for Wine.
    pub fn winetricks(self, build: Option<&WineBuild>) -> Result<Command> {
        let mut cmd = match build {
            Some(b) if b.kind == WineKind::Wine => {
                let winetricks = deps::require(
                    &["winetricks"],
                    "winetricks",
                    "install winetricks (Arch: pacman -S winetricks · Debian/Ubuntu: apt install winetricks)",
                )?;
                let mut cmd = Command::new(winetricks);
                cmd.env("WINE", b.wine_binary());
                cmd
            }
            _ => {
                let mut cmd = Command::new(&self.argv[0]);
                cmd.arg("winetricks");
                cmd
            }
        };
        cmd.arg("--gui");
        cmd.envs(self.envs);
        Ok(cmd)
    }
}

// ---- downloadable builds ----

#[derive(Debug, Clone, Copy)]
enum Checksum {
    /// `<asset stem>.sha512sum` next to each archive.
    Sha512Sidecar,
    /// One sha256 listing for every asset in the release.
    Sha256List(&'static str),
}

/// A GitHub project that publishes ready-to-run builds.
#[derive(Debug, Clone, Serialize)]
pub struct WineSource {
    pub id: &'static str,
    pub name: &'static str,
    pub kind: WineKind,
    pub description: &'static str,
    #[serde(skip)]
    repo: &'static str,
    /// The release asset to install ends with this…
    #[serde(skip)]
    asset_suffix: &'static str,
    /// …and contains none of these.
    #[serde(skip)]
    asset_exclude: &'static [&'static str],
    #[serde(skip)]
    checksum: Checksum,
}

pub const SOURCES: &[WineSource] = &[
    WineSource {
        id: "ge-proton",
        name: "GE-Proton",
        kind: WineKind::Proton,
        description: "Proton with extra media codecs and game fixes — the usual first pick",
        repo: "GloriousEggroll/proton-ge-custom",
        asset_suffix: ".tar.gz",
        asset_exclude: &["aarch64"],
        checksum: Checksum::Sha512Sidecar,
    },
    WineSource {
        id: "umu-proton",
        name: "UMU-Proton",
        kind: WineKind::Proton,
        description: "Valve's Proton with umu's fixes — what umu-run uses when nothing is picked",
        repo: "Open-Wine-Components/umu-proton",
        asset_suffix: ".tar.gz",
        asset_exclude: &[],
        checksum: Checksum::Sha512Sidecar,
    },
    WineSource {
        id: "proton-cachyos",
        name: "Proton-CachyOS",
        kind: WineKind::Proton,
        description: "CachyOS's Proton with performance patches",
        repo: "CachyOS/proton-cachyos",
        asset_suffix: "-x86_64.tar.xz",
        asset_exclude: &[],
        checksum: Checksum::Sha512Sidecar,
    },
    WineSource {
        id: "wine",
        name: "Wine",
        kind: WineKind::Wine,
        description: "Upstream Wine (Kron4ek build, WoW64 — no 32-bit system libraries needed)",
        repo: "Kron4ek/Wine-Builds",
        asset_suffix: "-amd64-wow64.tar.xz",
        asset_exclude: &["staging", "proton"],
        checksum: Checksum::Sha256List("sha256sums.txt"),
    },
    WineSource {
        id: "wine-staging-tkg",
        name: "Wine Staging-TkG",
        kind: WineKind::Wine,
        description: "Wine with the Staging and TkG patch sets (Kron4ek build, WoW64)",
        repo: "Kron4ek/Wine-Builds",
        asset_suffix: "-staging-tkg-amd64-wow64.tar.xz",
        asset_exclude: &[],
        checksum: Checksum::Sha256List("sha256sums.txt"),
    },
];

fn source(id: &str) -> Result<&'static WineSource> {
    SOURCES.iter().find(|s| s.id == id).ok_or_else(|| {
        let known: Vec<&str> = SOURCES.iter().map(|s| s.id).collect();
        CoreError::NotFound(format!(
            "unknown build source `{id}` (known: {})",
            known.join(", ")
        ))
    })
}

/// One downloadable release of a source.
#[derive(Debug, Clone, Serialize)]
pub struct WineRelease {
    pub source: &'static str,
    pub tag: String,
    pub asset: String,
    pub size: u64,
    pub published_at: Option<String>,
    pub prerelease: bool,
    /// A build named after this tag or asset is already installed.
    pub installed: bool,
}

#[derive(Debug, Deserialize)]
struct GhRelease {
    tag_name: String,
    #[serde(default)]
    prerelease: bool,
    #[serde(default)]
    draft: bool,
    published_at: Option<String>,
    assets: Vec<GhAsset>,
}

#[derive(Debug, Clone, Deserialize)]
struct GhAsset {
    name: String,
    size: u64,
    browser_download_url: String,
}

impl WineSource {
    fn pick_asset<'a>(&self, assets: &'a [GhAsset]) -> Option<&'a GhAsset> {
        assets.iter().find(|a| {
            a.name.ends_with(self.asset_suffix)
                && !self.asset_exclude.iter().any(|x| a.name.contains(x))
        })
    }

    fn checksum_asset_name(&self, asset: &str) -> String {
        match self.checksum {
            Checksum::Sha512Sidecar => format!("{}.sha512sum", archive_stem(asset)),
            Checksum::Sha256List(name) => name.to_string(),
        }
    }
}

/// `GE-Proton11-7-x86_64.tar.gz` → `GE-Proton11-7-x86_64`.
fn archive_stem(asset: &str) -> &str {
    [".tar.gz", ".tar.xz", ".tar.zst", ".tgz"]
        .iter()
        .find_map(|ext| asset.strip_suffix(ext))
        .unwrap_or(asset)
}

fn agent() -> ureq::Agent {
    // No overall timeout: builds are hundreds of MB. Stalls are caught by
    // the connect/response timeouts.
    ureq::Agent::config_builder()
        .timeout_connect(Some(Duration::from_secs(20)))
        .timeout_recv_response(Some(Duration::from_secs(30)))
        .user_agent(concat!("pezo/", env!("CARGO_PKG_VERSION")))
        .build()
        .into()
}

fn net_err(url: &str) -> impl FnOnce(ureq::Error) -> CoreError + '_ {
    move |e| CoreError::Network(format!("{url}: {e}"))
}

fn github_json<T: serde::de::DeserializeOwned>(agent: &ureq::Agent, url: &str) -> Result<T> {
    agent
        .get(url)
        .header("Accept", "application/vnd.github+json")
        .call()
        .map_err(net_err(url))?
        .body_mut()
        .read_json()
        .map_err(net_err(url))
}

/// Recent releases of a source that carry an installable asset, newest first.
pub fn list_releases(source_id: &str) -> Result<Vec<WineRelease>> {
    let src = source(source_id)?;
    let url = format!(
        "https://api.github.com/repos/{}/releases?per_page=25",
        src.repo
    );
    let releases: Vec<GhRelease> = github_json(&agent(), &url)?;
    let installed: Vec<String> = detect()?.into_iter().map(|b| b.name).collect();
    Ok(releases
        .into_iter()
        .filter(|r| !r.draft)
        .filter_map(|r| {
            let asset = src.pick_asset(&r.assets)?;
            let is_installed = installed
                .iter()
                .any(|n| *n == r.tag_name || n == archive_stem(&asset.name));
            Some(WineRelease {
                source: src.id,
                asset: asset.name.clone(),
                size: asset.size,
                tag: r.tag_name,
                published_at: r.published_at,
                prerelease: r.prerelease,
                installed: is_installed,
            })
        })
        .collect())
}

/// Install progress, reported as the download streams and the archive unpacks.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "stage", rename_all = "kebab-case")]
pub enum InstallProgress {
    Downloading { done: u64, total: u64 },
    Verifying,
    Extracting,
}

/// Removes the staging folder however the install ends.
struct Staging(PathBuf);

impl Drop for Staging {
    fn drop(&mut self) {
        if self.0.exists()
            && let Err(e) = fs::remove_dir_all(&self.0)
        {
            tracing::warn!(path = %self.0.display(), "failed to clean staging folder: {e}");
        }
    }
}

/// Download, verify and unpack a release (`tag` None → the latest stable
/// one). Proton lands in Steam's `compatibilitytools.d`, Wine in Pezo's data
/// folder.
pub fn install_release(
    source_id: &str,
    tag: Option<&str>,
    progress: &dyn Fn(InstallProgress),
) -> Result<WineBuild> {
    let src = source(source_id)?;
    let agent = agent();
    let url = match tag {
        Some(tag) => format!(
            "https://api.github.com/repos/{}/releases/tags/{tag}",
            src.repo
        ),
        None => format!("https://api.github.com/repos/{}/releases/latest", src.repo),
    };
    let release: GhRelease = github_json(&agent, &url)?;
    let asset = src.pick_asset(&release.assets).ok_or_else(|| {
        CoreError::NotFound(format!(
            "{} {} has no {} build to install",
            src.name, release.tag_name, src.asset_suffix
        ))
    })?;
    let checksum_name = src.checksum_asset_name(&asset.name);
    let checksum_asset = release
        .assets
        .iter()
        .find(|a| a.name == checksum_name)
        .ok_or_else(|| {
            CoreError::NotFound(format!(
                "{} {} publishes no checksum ({checksum_name}) — refusing an unverified download",
                src.name, release.tag_name
            ))
        })?;

    let target = match src.kind {
        WineKind::Proton => proton_install_dir(&data_home()?),
        WineKind::Wine => wine_install_dir(&data_home()?),
    };
    fs::create_dir_all(&target).map_err(io_err(&target))?;
    // Staged next to the destination so the final move is a same-disk rename.
    let staging = Staging(target.join(format!(".pezo-staging-{}", archive_stem(&asset.name))));
    if staging.0.exists() {
        fs::remove_dir_all(&staging.0).map_err(io_err(&staging.0))?;
    }
    fs::create_dir_all(&staging.0).map_err(io_err(&staging.0))?;

    let archive = staging.0.join(&asset.name);
    let digest = download(&agent, asset, &archive, src.checksum, progress)?;

    progress(InstallProgress::Verifying);
    let listing = agent
        .get(&checksum_asset.browser_download_url)
        .call()
        .map_err(net_err(&checksum_asset.browser_download_url))?
        .body_mut()
        .read_to_string()
        .map_err(net_err(&checksum_asset.browser_download_url))?;
    let expected = expected_digest(&listing, &asset.name).ok_or_else(|| {
        CoreError::InvalidState(format!("{checksum_name} has no entry for {}", asset.name))
    })?;
    if expected != digest {
        return Err(CoreError::InvalidState(format!(
            "checksum mismatch for {} — the download is corrupt or was tampered with",
            asset.name
        )));
    }

    progress(InstallProgress::Extracting);
    let unpacked = staging.0.join("unpacked");
    fs::create_dir_all(&unpacked).map_err(io_err(&unpacked))?;
    let tar = deps::require(&["tar", "bsdtar"], "tar", "install GNU tar or bsdtar")?;
    run_extractor(&mut deprioritized(
        &tar,
        &[
            "-xf".as_ref(),
            archive.as_os_str(),
            "-C".as_ref(),
            unpacked.as_os_str(),
        ],
    ))?;
    let root = single_build_dir(&unpacked, src.kind)?;
    let name = root
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .ok_or_else(|| CoreError::InvalidState(format!("bad build folder {}", root.display())))?;
    if detect()?.iter().any(|b| b.name == name) {
        return Err(CoreError::InvalidState(format!(
            "`{name}` is already installed"
        )));
    }
    let dest = target.join(&name);
    fs::rename(&root, &dest).map_err(io_err(&dest))?;
    drop(staging);

    tracing::info!(build = %name, path = %dest.display(), "installed wine build");
    Ok(WineBuild {
        name,
        kind: src.kind,
        path: dest,
        source: match src.kind {
            WineKind::Proton => BuildSource::CompatTools,
            WineKind::Wine => BuildSource::Pezo,
        },
        removable: true,
    })
}

/// Stream `asset` to `dest`, hashing as it lands. Returns the lowercase hex digest.
fn download(
    agent: &ureq::Agent,
    asset: &GhAsset,
    dest: &Path,
    checksum: Checksum,
    progress: &dyn Fn(InstallProgress),
) -> Result<String> {
    const REPORT_EVERY: u64 = 4 * 1024 * 1024;
    let url = &asset.browser_download_url;
    let response = agent.get(url).call().map_err(net_err(url))?;
    let total = response.body().content_length().unwrap_or(asset.size);
    let mut reader = response.into_body().into_reader();
    let mut file = File::create(dest).map_err(io_err(dest))?;
    let mut hasher = Hasher::new(checksum);
    let mut buf = vec![0u8; 256 * 1024];
    let (mut done, mut reported) = (0u64, 0u64);
    progress(InstallProgress::Downloading { done, total });
    loop {
        let n = reader
            .read(&mut buf)
            .map_err(|e| CoreError::Network(format!("{url}: {e}")))?;
        if n == 0 {
            break;
        }
        file.write_all(&buf[..n]).map_err(io_err(dest))?;
        hasher.update(&buf[..n]);
        done += n as u64;
        if done - reported >= REPORT_EVERY {
            reported = done;
            progress(InstallProgress::Downloading { done, total });
        }
    }
    file.flush().map_err(io_err(dest))?;
    if done != total {
        return Err(CoreError::Network(format!(
            "{url}: download ended after {done} of {total} bytes"
        )));
    }
    progress(InstallProgress::Downloading { done, total });
    Ok(hasher.hex())
}

enum Hasher {
    Sha256(Sha256),
    Sha512(Sha512),
}

impl Hasher {
    fn new(checksum: Checksum) -> Self {
        match checksum {
            Checksum::Sha512Sidecar => Self::Sha512(Sha512::new()),
            Checksum::Sha256List(_) => Self::Sha256(Sha256::new()),
        }
    }

    fn update(&mut self, data: &[u8]) {
        match self {
            Self::Sha256(h) => h.update(data),
            Self::Sha512(h) => h.update(data),
        }
    }

    fn hex(self) -> String {
        let bytes: Vec<u8> = match self {
            Self::Sha256(h) => h.finalize().to_vec(),
            Self::Sha512(h) => h.finalize().to_vec(),
        };
        bytes.iter().map(|b| format!("{b:02x}")).collect()
    }
}

/// Digest for `asset` in a `sha*sum`-style listing (`<hex>  <file>` lines).
fn expected_digest(listing: &str, asset: &str) -> Option<String> {
    listing.lines().find_map(|line| {
        let mut fields = line.split_whitespace();
        let hash = fields.next()?;
        let file = fields
            .next()?
            .trim_start_matches('*')
            .trim_start_matches("./");
        (file == asset).then(|| hash.to_ascii_lowercase())
    })
}

/// The one build folder an archive unpacked to.
fn single_build_dir(unpacked: &Path, kind: WineKind) -> Result<PathBuf> {
    let entries: Vec<PathBuf> = fs::read_dir(unpacked)
        .map_err(io_err(unpacked))?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .collect();
    match entries.as_slice() {
        [dir] if is_build(dir, kind) => Ok(dir.clone()),
        _ => Err(CoreError::InvalidState(format!(
            "the archive did not unpack to a single {} build folder",
            match kind {
                WineKind::Proton => "Proton",
                WineKind::Wine => "Wine",
            }
        ))),
    }
}

/// Compare names with digit runs as numbers: `GE-Proton10-9` < `GE-Proton10-14`.
fn natural_cmp(a: &str, b: &str) -> Ordering {
    let (mut a, mut b) = (a.as_bytes(), b.as_bytes());
    loop {
        match (a.first(), b.first()) {
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(x), Some(y)) if x.is_ascii_digit() && y.is_ascii_digit() => {
                let a_len = a.iter().take_while(|c| c.is_ascii_digit()).count();
                let b_len = b.iter().take_while(|c| c.is_ascii_digit()).count();
                let a_num = std::str::from_utf8(&a[..a_len])
                    .unwrap_or("0")
                    .trim_start_matches('0');
                let b_num = std::str::from_utf8(&b[..b_len])
                    .unwrap_or("0")
                    .trim_start_matches('0');
                let ord = a_num.len().cmp(&b_num.len()).then_with(|| a_num.cmp(b_num));
                if ord != Ordering::Equal {
                    return ord;
                }
                a = &a[a_len..];
                b = &b[b_len..];
            }
            (Some(x), Some(y)) => {
                let ord = x.to_ascii_lowercase().cmp(&y.to_ascii_lowercase());
                if ord != Ordering::Equal {
                    return ord;
                }
                a = &a[1..];
                b = &b[1..];
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn touch(path: &Path) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, b"").unwrap();
    }

    fn loc(dir: &Path, kind: WineKind, removable: bool) -> Location {
        Location {
            dir: dir.to_path_buf(),
            kind,
            source: BuildSource::CompatTools,
            removable,
        }
    }

    #[test]
    fn detects_builds_and_skips_non_builds() {
        let tmp = tempfile::tempdir().unwrap();
        let compat = tmp.path().join("compat");
        let wine = tmp.path().join("wine");
        touch(&compat.join("GE-Proton10-9/proton"));
        touch(&compat.join("GE-Proton10-14/proton"));
        touch(&compat.join("Steam Linux Runtime/run"));
        touch(&wine.join("wine-11.19-amd64-wow64/bin/wine"));
        touch(&wine.join("broken/lib/wine.so"));

        let builds = detect_in(
            &[
                loc(&compat, WineKind::Proton, true),
                loc(&wine, WineKind::Wine, false),
            ],
            Some(Path::new("/usr/bin/wine")),
        );
        let names: Vec<&str> = builds.iter().map(|b| b.name.as_str()).collect();
        assert_eq!(
            names,
            [
                "GE-Proton10-14",
                "GE-Proton10-9",
                "wine-11.19-amd64-wow64",
                "system"
            ]
        );
        let system = builds.last().unwrap();
        assert_eq!(system.path, PathBuf::from("/usr"));
        assert_eq!(system.wine_binary(), PathBuf::from("/usr/bin/wine"));
        assert!(!system.removable);
        assert!(builds[0].removable);
    }

    #[test]
    fn first_location_wins_on_duplicate_names() {
        let tmp = tempfile::tempdir().unwrap();
        let first = tmp.path().join("first");
        let second = tmp.path().join("second");
        touch(&first.join("GE-Proton11-1/proton"));
        touch(&second.join("GE-Proton11-1/proton"));
        let builds = detect_in(
            &[
                loc(&first, WineKind::Proton, true),
                loc(&second, WineKind::Proton, false),
            ],
            None,
        );
        assert_eq!(builds.len(), 1);
        assert!(builds[0].path.starts_with(&first));
    }

    #[test]
    fn symlinked_dirs_are_not_listed_twice() {
        let tmp = tempfile::tempdir().unwrap();
        let real = tmp.path().join("real");
        touch(&real.join("Proton-A/proton"));
        let link = tmp.path().join("link");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        // Same folder under a different build name can't happen; same path can.
        let builds = detect_in(
            &[
                loc(&link, WineKind::Proton, true),
                loc(&real, WineKind::Proton, true),
            ],
            None,
        );
        assert_eq!(builds.len(), 1);
    }

    #[test]
    fn picks_the_right_asset_per_source() {
        let asset = |name: &str| GhAsset {
            name: name.into(),
            size: 1,
            browser_download_url: String::new(),
        };
        let ge = [
            asset("GE-Proton11-7-aarch64.tar.gz"),
            asset("GE-Proton11-7-x86_64.sha512sum"),
            asset("GE-Proton11-7-x86_64.tar.gz"),
        ];
        let kron = [
            asset("sha256sums.txt"),
            asset("wine-11.19-amd64.tar.xz"),
            asset("wine-11.19-staging-amd64-wow64.tar.xz"),
            asset("wine-11.19-staging-tkg-amd64-wow64.tar.xz"),
            asset("wine-11.19-amd64-wow64.tar.xz"),
        ];
        let cachy = [
            asset("proton-cachyos-11.0-20261005-slr-x86_64_v3.tar.xz"),
            asset("proton-cachyos-11.0-20261005-slr-x86_64.tar.xz"),
        ];
        let pick = |id: &str, assets: &[GhAsset]| {
            source(id)
                .unwrap()
                .pick_asset(assets)
                .map(|a| a.name.clone())
        };
        assert_eq!(
            pick("ge-proton", &ge).as_deref(),
            Some("GE-Proton11-7-x86_64.tar.gz")
        );
        assert_eq!(
            pick("wine", &kron).as_deref(),
            Some("wine-11.19-amd64-wow64.tar.xz")
        );
        assert_eq!(
            pick("wine-staging-tkg", &kron).as_deref(),
            Some("wine-11.19-staging-tkg-amd64-wow64.tar.xz")
        );
        assert_eq!(
            pick("proton-cachyos", &cachy).as_deref(),
            Some("proton-cachyos-11.0-20261005-slr-x86_64.tar.xz")
        );
        assert_eq!(
            source("ge-proton")
                .unwrap()
                .checksum_asset_name("GE-Proton11-7-x86_64.tar.gz"),
            "GE-Proton11-7-x86_64.sha512sum"
        );
        assert_eq!(
            source("wine")
                .unwrap()
                .checksum_asset_name("wine-11.19-amd64-wow64.tar.xz"),
            "sha256sums.txt"
        );
    }

    #[test]
    fn parses_checksum_listings() {
        let listing = "AAA  wine-11.19-amd64.tar.xz\nbbb *./wine-11.19-amd64-wow64.tar.xz\n";
        assert_eq!(
            expected_digest(listing, "wine-11.19-amd64.tar.xz").as_deref(),
            Some("aaa")
        );
        assert_eq!(
            expected_digest(listing, "wine-11.19-amd64-wow64.tar.xz").as_deref(),
            Some("bbb")
        );
        assert_eq!(expected_digest(listing, "missing.tar.xz"), None);
    }

    #[test]
    fn hashes_match_coreutils() {
        let mut h = Hasher::new(Checksum::Sha256List("x"));
        h.update(b"abc");
        assert_eq!(
            h.hex(),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        let mut h = Hasher::new(Checksum::Sha512Sidecar);
        h.update(b"abc");
        assert!(h.hex().starts_with("ddaf35a193617aba"));
    }

    #[test]
    fn unpacked_archive_must_hold_one_build() {
        let tmp = tempfile::tempdir().unwrap();
        touch(&tmp.path().join("GE-Proton11-7/proton"));
        assert_eq!(
            single_build_dir(tmp.path(), WineKind::Proton).unwrap(),
            tmp.path().join("GE-Proton11-7")
        );
        assert!(single_build_dir(tmp.path(), WineKind::Wine).is_err());
        touch(&tmp.path().join("stray.txt"));
        assert!(single_build_dir(tmp.path(), WineKind::Proton).is_err());
    }

    #[test]
    fn wine_builds_run_their_own_binary() {
        let build = WineBuild {
            name: "wine-11.19-amd64-wow64".into(),
            kind: WineKind::Wine,
            path: PathBuf::from("/opt/wine"),
            source: BuildSource::Pezo,
            removable: true,
        };
        let cmd = WineCommand::new(Some(&build), Path::new("/p"), "umu-x", Some("gog")).unwrap();
        assert_eq!(cmd.argv, [OsString::from("/opt/wine/bin/wine")]);
        assert_eq!(
            cmd.envs,
            [
                ("WINEPREFIX", OsString::from("/p")),
                ("WINEDLLOVERRIDES", OsString::from("winemenubuilder.exe=d")),
            ]
        );
    }

    #[test]
    fn natural_order() {
        assert_eq!(
            natural_cmp("GE-Proton10-9", "GE-Proton10-14"),
            Ordering::Less
        );
        assert_eq!(natural_cmp("wine-9.0", "wine-11.19"), Ordering::Less);
        assert_eq!(natural_cmp("a", "a"), Ordering::Equal);
    }

    #[test]
    fn archive_stems() {
        assert_eq!(
            archive_stem("GE-Proton11-7-x86_64.tar.gz"),
            "GE-Proton11-7-x86_64"
        );
        assert_eq!(archive_stem("wine-11.19-amd64.tar.xz"), "wine-11.19-amd64");
    }

    #[test]
    #[ignore = "hits the live GitHub API"]
    fn lists_real_releases() {
        for src in SOURCES {
            let releases = list_releases(src.id).expect("list releases");
            assert!(
                !releases.is_empty(),
                "{} has no installable releases",
                src.id
            );
        }
    }
}
