use std::env;
use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::error::{CoreError, Result, io_err};

pub const DB_FILE: &str = "pezo.db";
pub const INSTALLERS_DIR: &str = "installers";
pub const GAMES_DIR: &str = "library";
pub const PREFIXES_DIR: &str = "prefixes";

/// Persistent app configuration, stored at `$XDG_CONFIG_HOME/pezo/config.json`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Config {
    /// Root of the game library disk, e.g. `/mnt/games`.
    pub library_root: Option<PathBuf>,
    /// Extra folders scanned for installers, e.g. other disks. Unlike the
    /// library root, nothing is installed to or written in these.
    #[serde(default)]
    pub scan_dirs: Vec<PathBuf>,
    /// Copy portable games into the library on install instead of running
    /// them from wherever they were found (survives unplugging that disk).
    #[serde(default = "default_true")]
    pub adopt_portables: bool,
}

fn default_true() -> bool {
    true
}

impl Config {
    pub fn path() -> Result<PathBuf> {
        let base = env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
            .ok_or_else(|| CoreError::Config("neither XDG_CONFIG_HOME nor HOME is set".into()))?;
        Ok(base.join("pezo").join("config.json"))
    }

    pub fn load() -> Result<Self> {
        let path = Self::path()?;
        match fs::read(&path) {
            Ok(bytes) => serde_json::from_slice(&bytes)
                .map_err(|e| CoreError::Config(format!("failed to parse {}: {e}", path.display()))),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(source) => Err(CoreError::Io { path, source }),
        }
    }

    pub fn save(&self) -> Result<()> {
        let path = Self::path()?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(io_err(parent))?;
        }
        let bytes = serde_json::to_vec_pretty(self)
            .map_err(|e| CoreError::Config(format!("failed to serialize config: {e}")))?;
        fs::write(&path, bytes).map_err(io_err(&path))
    }

    /// Resolve the library layout; errors if no library root is configured.
    pub fn library(&self) -> Result<LibraryPaths> {
        let root = self.library_root.clone().ok_or_else(|| {
            CoreError::Config(
                "library root is not set — run `pezo init <path>` or pick a folder in the app"
                    .into(),
            )
        })?;
        Ok(LibraryPaths::new(root))
    }
}

/// Directory layout inside the library root. The whole library — installers,
/// installed games, wine prefixes and the database — lives on one disk so it
/// stays portable across machines and reinstalls.
#[derive(Debug, Clone)]
pub struct LibraryPaths {
    pub root: PathBuf,
    pub installers: PathBuf,
    pub games: PathBuf,
    pub prefixes: PathBuf,
    pub db_file: PathBuf,
}

impl LibraryPaths {
    pub fn new(root: PathBuf) -> Self {
        Self {
            installers: root.join(INSTALLERS_DIR),
            games: root.join(GAMES_DIR),
            prefixes: root.join(PREFIXES_DIR),
            db_file: root.join(DB_FILE),
            root,
        }
    }

    pub fn ensure_layout(&self) -> Result<()> {
        for dir in [&self.root, &self.installers, &self.games, &self.prefixes] {
            fs::create_dir_all(dir).map_err(io_err(dir))?;
        }
        Ok(())
    }

    /// Directory where fetched cover art is cached.
    pub fn covers(&self) -> PathBuf {
        self.root.join(".covers")
    }
}

/// Filesystem the library root sits on, and whether Wine/Proton will
/// misbehave there (NTFS and FAT variants can't hold prefixes reliably).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FsInfo {
    pub fstype: String,
    pub problematic: bool,
}

/// Look up the filesystem type of `path` via /proc/self/mounts, matching the
/// longest mount point that prefixes it. None if it can't be determined.
pub fn fs_info(path: &std::path::Path) -> Option<FsInfo> {
    let canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let mounts = fs::read_to_string("/proc/self/mounts").ok()?;
    let mut best: Option<(usize, String)> = None;
    for line in mounts.lines() {
        let mut fields = line.split_whitespace();
        let (Some(_dev), Some(mount), Some(fstype)) =
            (fields.next(), fields.next(), fields.next())
        else {
            continue;
        };
        // Spaces in mount points appear octal-escaped (\040) in /proc/mounts.
        let mount = unescape_mount(mount);
        let mount_len = mount.as_os_str().len();
        if canonical.starts_with(&mount) && best.as_ref().is_none_or(|(len, _)| mount_len > *len) {
            best = Some((mount_len, fstype.to_string()));
        }
    }
    let (_, fstype) = best?;
    // ntfs-3g mounts report as "fuseblk"; ntfs3 (kernel) as "ntfs3".
    let problematic = matches!(
        fstype.as_str(),
        "ntfs" | "ntfs3" | "fuseblk" | "exfat" | "vfat" | "msdos"
    );
    Some(FsInfo { fstype, problematic })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fs_info_resolves_root() {
        let info = fs_info(std::path::Path::new("/")).expect("root fs");
        assert!(!info.fstype.is_empty());
    }

    #[test]
    fn unescapes_octal_mount_paths() {
        assert_eq!(
            unescape_mount("/run/media/user/WD\\0406TB"),
            PathBuf::from("/run/media/user/WD 6TB")
        );
        assert_eq!(unescape_mount("/plain"), PathBuf::from("/plain"));
    }
}

fn unescape_mount(s: &str) -> PathBuf {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\' {
            let code: String = chars.by_ref().take(3).collect();
            if let Ok(n) = u8::from_str_radix(&code, 8) {
                out.push(n as char);
                continue;
            }
            out.push(c);
            out.push_str(&code);
        } else {
            out.push(c);
        }
    }
    PathBuf::from(out)
}
