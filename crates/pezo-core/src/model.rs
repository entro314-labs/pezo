use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::error::{CoreError, Result};

/// What a scanned filesystem item was classified as.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CandidateKind {
    /// GOG offline installer (Inno Setup with GOG markers). Installable via innoextract.
    GogInstaller,
    /// Generic Inno Setup installer. Installable via innoextract.
    InnoInstaller,
    /// Windows installer (.exe/.msi) that must be run inside a wine prefix.
    WindowsInstaller,
    /// Folder (or single exe) that runs in place under Proton.
    PortableWindows,
    /// DOS-era game folder, runs under DOSBox.
    DosGame,
    /// ScummVM-engine game folder.
    ScummVmGame,
    /// Archive (zip/7z/tar/…) to extract, then re-classify.
    Archive,
    /// Recognized as a game-ish item but no strategy applies.
    Unknown,
}

impl CandidateKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::GogInstaller => "gog-installer",
            Self::InnoInstaller => "inno-installer",
            Self::WindowsInstaller => "windows-installer",
            Self::PortableWindows => "portable-windows",
            Self::DosGame => "dos-game",
            Self::ScummVmGame => "scummvm-game",
            Self::Archive => "archive",
            Self::Unknown => "unknown",
        }
    }

    pub fn parse(s: &str) -> Result<Self> {
        match s {
            "gog-installer" => Ok(Self::GogInstaller),
            "inno-installer" => Ok(Self::InnoInstaller),
            "windows-installer" => Ok(Self::WindowsInstaller),
            "portable-windows" => Ok(Self::PortableWindows),
            "dos-game" => Ok(Self::DosGame),
            "scummvm-game" => Ok(Self::ScummVmGame),
            "archive" => Ok(Self::Archive),
            "unknown" => Ok(Self::Unknown),
            other => Err(CoreError::InvalidState(format!(
                "unknown candidate kind `{other}` in database"
            ))),
        }
    }
}

/// How an installed game is launched.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Runner {
    /// Proton via umu-launcher.
    Proton,
    Dosbox,
    Scummvm,
}

impl Runner {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Proton => "proton",
            Self::Dosbox => "dosbox",
            Self::Scummvm => "scummvm",
        }
    }

    pub fn parse(s: &str) -> Result<Self> {
        match s {
            "proton" => Ok(Self::Proton),
            "dosbox" => Ok(Self::Dosbox),
            "scummvm" => Ok(Self::Scummvm),
            other => Err(CoreError::InvalidState(format!(
                "unknown runner `{other}` in database"
            ))),
        }
    }
}

/// A scanned, not-necessarily-installed item found on the library disk.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Candidate {
    pub id: i64,
    pub path: PathBuf,
    pub kind: CandidateKind,
    pub display_name: String,
    pub size_bytes: u64,
    pub discovered_at: String,
    pub installed_game_id: Option<i64>,
}

/// An installed, launchable game.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Game {
    pub id: i64,
    pub slug: String,
    pub title: String,
    pub runner: Runner,
    /// Directory the game lives in.
    pub install_path: PathBuf,
    /// Launch target: exe for Proton, conf file for DOSBox, game dir for ScummVM.
    pub launch_path: PathBuf,
    /// Wine prefix directory (Proton games only).
    pub prefix_path: Option<PathBuf>,
    /// Original installer/source path, if installed from a candidate.
    pub source_path: Option<PathBuf>,
    pub added_at: String,
    pub last_played_at: Option<String>,
    pub play_seconds: i64,
    /// Store-specific game id for umu's protonfixes (e.g. GOG product id,
    /// Steam appid). None → generic `umu-<slug>` handling.
    pub store_id: Option<String>,
    /// Which store the id belongs to ("gog", "steam"), for umu's STORE env.
    pub store: Option<String>,
    /// Extra command-line arguments appended at launch.
    pub launch_args: String,
    /// Extra environment, one KEY=VALUE per line.
    pub env_vars: String,
    /// Comma-separated launch wrappers ("gamemode", "mangohud").
    pub wrappers: String,
}

/// Fields required to register a new game.
#[derive(Debug, Clone)]
pub struct NewGame<'a> {
    pub slug: &'a str,
    pub title: &'a str,
    pub runner: Runner,
    pub install_path: &'a std::path::Path,
    pub launch_path: &'a std::path::Path,
    pub prefix_path: Option<&'a std::path::Path>,
    pub source_path: Option<&'a std::path::Path>,
    pub store_id: Option<String>,
    pub store: Option<&'a str>,
}
