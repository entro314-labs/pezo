use std::env;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{CoreError, Result};

/// Status of one external tool Pezo orchestrates.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolStatus {
    pub name: String,
    pub found: bool,
    pub path: Option<PathBuf>,
    pub purpose: String,
    pub hint: String,
    pub required: bool,
}

struct ToolSpec {
    bins: &'static [&'static str],
    name: &'static str,
    purpose: &'static str,
    hint: &'static str,
    required: bool,
}

const TOOLS: &[ToolSpec] = &[
    ToolSpec {
        bins: &["umu-run"],
        name: "umu-launcher",
        purpose: "Runs Windows games with Proton outside Steam",
        hint: "Arch: pacman -S umu-launcher · Fedora: dnf install umu-launcher · elsewhere: https://github.com/Open-Wine-Components/umu-launcher",
        required: true,
    },
    ToolSpec {
        bins: &["innoextract"],
        name: "innoextract",
        purpose: "Unpacks GOG offline installers without running them",
        hint: "Arch: pacman -S innoextract · Debian/Ubuntu: apt install innoextract · macOS: brew install innoextract",
        required: true,
    },
    ToolSpec {
        bins: &["bsdtar"],
        name: "bsdtar (libarchive)",
        purpose: "Extracts zip/7z/tar archives",
        hint: "Arch: pacman -S libarchive · Debian/Ubuntu: apt install libarchive-tools · macOS: ships with the OS",
        required: false,
    },
    ToolSpec {
        bins: &["7z", "7zz", "7za"],
        name: "7-Zip",
        purpose: "Extracts split archives (.part1.rar, .7z.001) and stubborn RARs",
        hint: "Arch: pacman -S 7zip · Debian/Ubuntu: apt install 7zip · macOS: brew install sevenzip",
        required: false,
    },
    ToolSpec {
        bins: &["unrar"],
        name: "unrar",
        purpose: "Fallback extractor for RAR archives 7-Zip can't read",
        hint: "Arch: pacman -S unrar · Debian/Ubuntu: apt install unrar",
        required: false,
    },
    ToolSpec {
        bins: &["dosbox-staging", "dosbox"],
        name: "DOSBox",
        purpose: "Runs DOS-era games",
        hint: "Arch: pacman -S dosbox-staging · Debian/Ubuntu: apt install dosbox-staging · macOS: brew install dosbox-staging",
        required: false,
    },
    ToolSpec {
        bins: &["scummvm"],
        name: "ScummVM",
        purpose: "Runs classic adventure-engine games",
        hint: "Arch: pacman -S scummvm · Debian/Ubuntu: apt install scummvm · macOS: brew install scummvm",
        required: false,
    },
    ToolSpec {
        bins: &["gamemoderun"],
        name: "GameMode",
        purpose: "CPU/GPU performance governor while a game runs (per-game toggle)",
        hint: "Arch: pacman -S gamemode · Debian/Ubuntu: apt install gamemode",
        required: false,
    },
    ToolSpec {
        bins: &["mangohud"],
        name: "MangoHud",
        purpose: "FPS / frametime overlay (per-game toggle)",
        hint: "Arch: pacman -S mangohud · Debian/Ubuntu: apt install mangohud",
        required: false,
    },
    ToolSpec {
        bins: &["winetricks"],
        name: "winetricks",
        purpose: "Installs runtimes (DXVK, fonts, VC++) into a game's prefix",
        hint: "Arch: pacman -S winetricks · Debian/Ubuntu: apt install winetricks",
        required: false,
    },
];

/// Locate a binary on PATH.
pub fn which(bin: &str) -> Option<PathBuf> {
    let path_var = env::var_os("PATH")?;
    env::split_paths(&path_var)
        .map(|dir| dir.join(bin))
        .find(|candidate| is_executable(candidate))
}

/// Locate the first available binary from a list of alternatives.
pub fn which_any(bins: &[&str]) -> Option<PathBuf> {
    bins.iter().find_map(|b| which(b))
}

/// Locate a required tool or fail with an actionable hint.
pub fn require(bins: &[&str], name: &str, hint: &str) -> Result<PathBuf> {
    which_any(bins).ok_or_else(|| CoreError::MissingTool {
        tool: name.to_string(),
        hint: hint.to_string(),
    })
}

/// Report the status of every external tool Pezo uses.
pub fn doctor() -> Vec<ToolStatus> {
    TOOLS
        .iter()
        .map(|spec| {
            let path = which_any(spec.bins);
            ToolStatus {
                name: spec.name.to_string(),
                found: path.is_some(),
                path,
                purpose: spec.purpose.to_string(),
                hint: spec.hint.to_string(),
                required: spec.required,
            }
        })
        .collect()
}

#[cfg(unix)]
fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    path.metadata()
        .map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

#[cfg(not(unix))]
fn is_executable(path: &Path) -> bool {
    path.is_file()
}
