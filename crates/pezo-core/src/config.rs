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
}
