use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

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

/// Role of a file in a split archive set (`x.part2.rar`, `x.7z.002`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MultipartRole {
    /// First part — stands in for the whole set as one candidate.
    Head,
    /// Continuation part — never a candidate of its own.
    Tail,
}

/// Detect split-archive naming: `x.partN.rar` or `x.<archive-ext>.NNN`.
pub fn multipart_role(name: &str) -> Option<MultipartRole> {
    let lower = name.to_ascii_lowercase();
    if let Some(stem) = lower.strip_suffix(".rar") {
        if let Some(idx) = stem.rfind(".part") {
            let digits = &stem[idx + 5..];
            if !digits.is_empty() && digits.chars().all(|c| c.is_ascii_digit()) {
                return Some(if digits.trim_start_matches('0') == "1" {
                    MultipartRole::Head
                } else {
                    MultipartRole::Tail
                });
            }
        }
        return None;
    }
    if let Some((stem, digits)) = lower.rsplit_once('.') {
        if digits.len() == 3
            && digits.chars().all(|c| c.is_ascii_digit())
            && stem
                .rsplit_once('.')
                .is_some_and(|(_, ext)| ARCHIVE_EXTS.contains(&ext))
        {
            return Some(if digits == "001" {
                MultipartRole::Head
            } else {
                MultipartRole::Tail
            });
        }
    }
    None
}

/// All on-disk parts of a split archive, in order, given any part of it.
/// Returns just `path` itself for single-file archives.
pub fn multipart_parts(path: &Path) -> Vec<PathBuf> {
    let Some(parent) = path.parent() else {
        return vec![path.to_path_buf()];
    };
    let name = file_name_lower(path);
    // Byte length is stable under to_ascii_lowercase, so index math is safe.
    let base_len = if let Some(stem) = name.strip_suffix(".rar") {
        match stem.rfind(".part") {
            Some(idx) if stem[idx + 5..].chars().all(|c| c.is_ascii_digit()) => idx,
            _ => return vec![path.to_path_buf()],
        }
    } else if multipart_role(&name).is_some() {
        name.rfind('.').unwrap_or(name.len())
    } else {
        return vec![path.to_path_buf()];
    };
    let base = &name[..base_len];

    let Ok(entries) = fs::read_dir(parent) else {
        return vec![path.to_path_buf()];
    };
    let mut parts: Vec<(u32, PathBuf)> = entries
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter_map(|p| {
            let n = file_name_lower(&p);
            let number = part_number(&n, base)?;
            Some((number, p))
        })
        .collect();
    parts.sort();
    if parts.is_empty() {
        vec![path.to_path_buf()]
    } else {
        parts.into_iter().map(|(_, p)| p).collect()
    }
}

/// Part number of `name` if it belongs to the multipart set with prefix `base`.
fn part_number(name: &str, base: &str) -> Option<u32> {
    let rest = name.strip_prefix(base)?;
    if let Some(digits) = rest.strip_prefix(".part").and_then(|r| r.strip_suffix(".rar")) {
        return digits.parse().ok();
    }
    rest.strip_prefix('.')
        .filter(|d| d.len() == 3)
        .and_then(|d| d.parse().ok())
}

/// Part numbers missing from a multipart set (gaps before the last part).
pub fn multipart_gaps(head: &Path) -> Vec<u32> {
    let numbers: Vec<u32> = multipart_parts(head)
        .iter()
        .filter_map(|p| {
            let n = file_name_lower(p);
            let base_end = if n.ends_with(".rar") {
                n.rfind(".part")
            } else {
                n.rfind('.')
            }?;
            part_number(&n, &n[..base_end])
        })
        .collect();
    match numbers.iter().max() {
        Some(&max) => (1..=max).filter(|n| !numbers.contains(n)).collect(),
        None => Vec::new(),
    }
}

/// Downloader debris that should never drive classification: saved web pages,
/// scraper metadata, cover art, checksums, torrents.
pub fn is_junk_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    // Saved-webpage asset folders ("Page Title_files/").
    if lower.ends_with("_files") {
        return true;
    }
    let Some((stem, ext)) = lower.rsplit_once('.') else {
        return false;
    };
    const JUNK_EXTS: &[&str] = &[
        "jpg", "jpeg", "png", "gif", "webp", "bmp", "ico", "html", "htm", "mhtml", "pdf", "txt",
        "nfo", "diz", "url", "torrent", "sha1", "md5", "sfv", "json", "xml", "sqlite", "srt",
    ];
    JUNK_EXTS.contains(&ext) || stem.ends_with("_meta")
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

    match multipart_role(&name) {
        Some(MultipartRole::Head) => return Ok(Classification::Candidate(CandidateKind::Archive)),
        Some(MultipartRole::Tail) => return Ok(Classification::Ignore),
        None => {}
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
    // Steam library rips keep the game under steamapps/common/<Game>/, one
    // level deeper than the walk above reaches.
    if steam_common_dir(dir).is_some_and(|common| dir_has_exe(&common, 4)) {
        return Ok(Classification::Candidate(CandidateKind::PortableWindows));
    }
    // Downloaded rips often wrap one archive in a folder of covers/metadata.
    if find_inner_archive(dir).is_some() {
        return Ok(Classification::Candidate(CandidateKind::Archive));
    }
    Ok(Classification::Candidate(CandidateKind::Unknown))
}

/// `<dir>/steamapps/common` if present (Steam library rips).
pub fn steam_common_dir(dir: &Path) -> Option<PathBuf> {
    let common = dir.join("steamapps").join("common");
    common.is_dir().then_some(common)
}

fn dir_has_exe(dir: &Path, depth: usize) -> bool {
    WalkDir::new(dir)
        .max_depth(depth)
        .follow_links(false)
        .into_iter()
        .filter_map(|e| e.ok())
        .any(|e| {
            e.file_type().is_file()
                && e.file_name()
                    .to_string_lossy()
                    .to_ascii_lowercase()
                    .ends_with(".exe")
        })
}

/// The meaningful archives at the top level of a folder, once junk (covers,
/// saved pages, metadata) and multipart tails are set aside. Multipart sets
/// appear once, as their head.
pub fn list_dir_archives(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut archives = Vec::new();
    for entry in entries.filter_map(|e| e.ok()) {
        let path = entry.path();
        let name = file_name_lower(&path);
        if name.starts_with('.') || is_junk_name(&name) || path.is_dir() {
            continue;
        }
        match multipart_role(&name) {
            Some(MultipartRole::Head) => archives.push(path),
            Some(MultipartRole::Tail) => {}
            None => {
                if ARCHIVE_EXTS.contains(&extension_lower(&path).as_str()) {
                    archives.push(path);
                }
            }
        }
    }
    archives.sort();
    archives
}

/// The single meaningful archive inside a wrapper folder. None if the folder
/// holds zero or several archives — several is ambiguous (disc sets), so the
/// folder stays unclassified rather than guessing.
pub fn find_inner_archive(dir: &Path) -> Option<PathBuf> {
    match list_dir_archives(dir).as_slice() {
        [only] => Some(only.clone()),
        _ => None,
    }
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
    fn multipart_heads_are_candidates_tails_ignored() {
        let dir = tempfile::tempdir().expect("tempdir");
        let head = dir.path().join("game.part1.rar");
        let tail = dir.path().join("game.part2.rar");
        touch(&head, b"Rar!");
        touch(&tail, b"Rar!");
        assert_eq!(
            classify(&head).expect("classify"),
            Classification::Candidate(CandidateKind::Archive)
        );
        assert_eq!(classify(&tail).expect("classify"), Classification::Ignore);

        let head7z = dir.path().join("other.7z.001");
        let tail7z = dir.path().join("other.7z.002");
        touch(&head7z, b"7z");
        touch(&tail7z, b"7z");
        assert_eq!(
            classify(&head7z).expect("classify"),
            Classification::Candidate(CandidateKind::Archive)
        );
        assert_eq!(classify(&tail7z).expect("classify"), Classification::Ignore);
    }

    #[test]
    fn multipart_parts_and_gaps() {
        let dir = tempfile::tempdir().expect("tempdir");
        for n in [1, 2, 4] {
            touch(&dir.path().join(format!("game.7z.{n:03}")), b"7z");
        }
        let head = dir.path().join("game.7z.001");
        assert_eq!(multipart_parts(&head).len(), 3);
        assert_eq!(multipart_gaps(&head), vec![3]);

        let single = dir.path().join("whole.zip");
        touch(&single, b"PK");
        assert_eq!(multipart_parts(&single), vec![single.clone()]);
        assert!(multipart_gaps(&single).is_empty());
    }

    #[test]
    fn classifies_steam_library_rip_as_portable() {
        let dir = tempfile::tempdir().expect("tempdir");
        let game = dir.path().join("SomeRip");
        touch(
            &game
                .join("steamapps")
                .join("common")
                .join("The Game")
                .join("game.exe"),
            b"MZ",
        );
        touch(&game.join("steamapps").join("appmanifest_1.acf"), b"{}");
        assert_eq!(
            classify(&game).expect("classify"),
            Classification::Candidate(CandidateKind::PortableWindows)
        );
    }

    #[test]
    fn classifies_folder_wrapping_single_archive() {
        let dir = tempfile::tempdir().expect("tempdir");
        let rip = dir.path().join("Some Old Game (USA) PC");
        touch(&rip.join("Some Old Game.7z"), b"7z");
        touch(&rip.join("box1.jpg"), b"img");
        touch(&rip.join("someoldgame_meta.xml"), b"<x/>");
        touch(&rip.join("someoldgame_meta.sqlite"), b"db");
        touch(&rip.join("Page Download - Site.html"), b"<html>");
        assert_eq!(
            classify(&rip).expect("classify"),
            Classification::Candidate(CandidateKind::Archive)
        );
        assert_eq!(
            find_inner_archive(&rip),
            Some(rip.join("Some Old Game.7z"))
        );

        // Several archives (disc sets) stay unclassified rather than guessing.
        let discs = dir.path().join("Disc Set");
        touch(&discs.join("Disc 1.zip"), b"PK");
        touch(&discs.join("Disc 2.zip"), b"PK");
        assert_eq!(
            classify(&discs).expect("classify"),
            Classification::Candidate(CandidateKind::Unknown)
        );
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
