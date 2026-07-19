use std::fs;
use std::io::Read;
use std::path::Path;

use walkdir::WalkDir;

use crate::error::{Result, io_err};
use crate::model::CandidateKind;

/// How many leading bytes of an .exe are searched for installer signatures.
/// Inno Setup and GOG markers sit in the setup loader, well within this window.
const EXE_SCAN_WINDOW: usize = 8 * 1024 * 1024;

const ARCHIVE_EXTS: &[&str] = &["zip", "7z", "rar", "tar", "gz", "bz2", "xz", "zst", "iso"];

/// Result of classifying one filesystem entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Classification {
    Candidate(CandidateKind),
    /// Not a standalone item (multipart .bin data, hidden files, junk).
    Ignore,
}

/// Classify a top-level entry of the installers directory (file or folder).
pub fn classify(path: &Path) -> Result<Classification> {
    let name = file_name_lower(path);
    if name.starts_with('.') {
        return Ok(Classification::Ignore);
    }

    if path.is_dir() {
        return classify_dir(path);
    }

    let ext = extension_lower(path);
    match ext.as_str() {
        // Multipart installer data; belongs to a sibling setup exe.
        "bin" => Ok(Classification::Ignore),
        "exe" => classify_exe(path).map(Classification::Candidate),
        "msi" => Ok(Classification::Candidate(CandidateKind::WindowsInstaller)),
        e if ARCHIVE_EXTS.contains(&e) => Ok(Classification::Candidate(CandidateKind::Archive)),
        _ => Ok(Classification::Ignore),
    }
}

/// Classify a directory. Also used after extraction/installation to pick a runner.
pub fn classify_dir(dir: &Path) -> Result<Classification> {
    let mut has_exe = false;
    let mut has_installer_exe = false;
    let mut has_dosbox_marker = false;
    let mut has_scummvm_marker = false;

    for entry in WalkDir::new(dir)
        .max_depth(3)
        .follow_links(false)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        let name = entry.file_name().to_string_lossy().to_lowercase();
        if entry.file_type().is_dir() {
            if name == "dosbox" {
                has_dosbox_marker = true;
            }
            if name == "scummvm" {
                has_scummvm_marker = true;
            }
            continue;
        }
        if name.ends_with(".conf") && name.contains("dosbox") || name == "dosbox.conf" {
            has_dosbox_marker = true;
        }
        if name.ends_with(".scummvm") {
            has_scummvm_marker = true;
        }
        if name.ends_with(".exe") {
            has_exe = true;
            if name.starts_with("setup_") || name.starts_with("setup ") || name == "setup.exe" {
                has_installer_exe = true;
            }
        }
    }

    if has_dosbox_marker {
        return Ok(Classification::Candidate(CandidateKind::DosGame));
    }
    if has_scummvm_marker {
        return Ok(Classification::Candidate(CandidateKind::ScummVmGame));
    }
    if has_installer_exe {
        // A folder holding an installer (e.g. multi-part GOG download): classify
        // by the installer exe it contains.
        if let Some(setup) = find_setup_exe(dir) {
            return classify_exe(&setup).map(Classification::Candidate);
        }
    }
    if has_exe {
        return Ok(Classification::Candidate(CandidateKind::PortableWindows));
    }
    Ok(Classification::Candidate(CandidateKind::Unknown))
}

/// Find the setup exe inside a directory (used for folder-wrapped installers).
pub fn find_setup_exe(dir: &Path) -> Option<std::path::PathBuf> {
    WalkDir::new(dir)
        .max_depth(2)
        .follow_links(false)
        .into_iter()
        .filter_map(|e| e.ok())
        .find(|e| {
            let name = e.file_name().to_string_lossy().to_lowercase();
            e.file_type().is_file()
                && name.ends_with(".exe")
                && (name.starts_with("setup_") || name.starts_with("setup ") || name == "setup.exe")
        })
        .map(|e| e.into_path())
}

fn classify_exe(path: &Path) -> Result<CandidateKind> {
    let head = read_head(path, EXE_SCAN_WINDOW)?;
    let name = file_name_lower(path);
    let is_inno = contains(&head, b"Inno Setup");

    if is_inno {
        let looks_gog = name.starts_with("setup_") || contains(&head, b"GOG.com");
        return Ok(if looks_gog {
            CandidateKind::GogInstaller
        } else {
            CandidateKind::InnoInstaller
        });
    }

    // Non-Inno exe: either some other installer tech or a bare game exe.
    let installer_name = name.contains("setup") || name.contains("install");
    Ok(if installer_name {
        CandidateKind::WindowsInstaller
    } else {
        CandidateKind::PortableWindows
    })
}

fn read_head(path: &Path, limit: usize) -> Result<Vec<u8>> {
    let file = fs::File::open(path).map_err(io_err(path))?;
    let mut buf = Vec::with_capacity(limit.min(1024 * 1024));
    file.take(limit as u64)
        .read_to_end(&mut buf)
        .map_err(io_err(path))?;
    Ok(buf)
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    if needle.is_empty() || haystack.len() < needle.len() {
        return false;
    }
    haystack.windows(needle.len()).any(|w| w == needle)
}

fn file_name_lower(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().to_lowercase())
        .unwrap_or_default()
}

fn extension_lower(path: &Path) -> String {
    path.extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default()
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
    fn classifies_gog_inno_installer() {
        let dir = tempfile::tempdir().expect("tempdir");
        let exe = dir.path().join("setup_the_witness_1.0.exe");
        touch(&exe, b"MZ....Inno Setup Setup Data (5.5.7)....GOG.com....");
        assert_eq!(
            classify(&exe).expect("classify"),
            Classification::Candidate(CandidateKind::GogInstaller)
        );
    }

    #[test]
    fn classifies_generic_inno_installer() {
        let dir = tempfile::tempdir().expect("tempdir");
        let exe = dir.path().join("some_tool.exe");
        touch(&exe, b"MZ....Inno Setup Setup Data....");
        assert_eq!(
            classify(&exe).expect("classify"),
            Classification::Candidate(CandidateKind::InnoInstaller)
        );
    }

    #[test]
    fn classifies_bare_exe_as_portable() {
        let dir = tempfile::tempdir().expect("tempdir");
        let exe = dir.path().join("game.exe");
        touch(&exe, b"MZ....plain windows program....");
        assert_eq!(
            classify(&exe).expect("classify"),
            Classification::Candidate(CandidateKind::PortableWindows)
        );
    }

    #[test]
    fn ignores_bin_parts_and_hidden_files() {
        let dir = tempfile::tempdir().expect("tempdir");
        let bin = dir.path().join("setup_game-1.bin");
        touch(&bin, b"data");
        assert_eq!(classify(&bin).expect("classify"), Classification::Ignore);
        let hidden = dir.path().join(".DS_Store");
        touch(&hidden, b"junk");
        assert_eq!(classify(&hidden).expect("classify"), Classification::Ignore);
    }

    #[test]
    fn classifies_dosbox_folder() {
        let dir = tempfile::tempdir().expect("tempdir");
        let game = dir.path().join("Tyrian");
        touch(&game.join("dosbox.conf"), b"[autoexec]\n");
        touch(&game.join("TYRIAN.EXE"), b"MZ");
        assert_eq!(
            classify(&game).expect("classify"),
            Classification::Candidate(CandidateKind::DosGame)
        );
    }

    #[test]
    fn classifies_scummvm_folder() {
        let dir = tempfile::tempdir().expect("tempdir");
        let game = dir.path().join("Beneath a Steel Sky");
        touch(&game.join("sky.scummvm"), b"");
        assert_eq!(
            classify(&game).expect("classify"),
            Classification::Candidate(CandidateKind::ScummVmGame)
        );
    }

    #[test]
    fn classifies_portable_folder() {
        let dir = tempfile::tempdir().expect("tempdir");
        let game = dir.path().join("SomeGame");
        touch(&game.join("bin").join("game.exe"), b"MZ");
        assert_eq!(
            classify(&game).expect("classify"),
            Classification::Candidate(CandidateKind::PortableWindows)
        );
    }

    #[test]
    fn classifies_archive() {
        let dir = tempfile::tempdir().expect("tempdir");
        let zip = dir.path().join("oldgame.zip");
        touch(&zip, b"PK");
        assert_eq!(
            classify(&zip).expect("classify"),
            Classification::Candidate(CandidateKind::Archive)
        );
    }
}
