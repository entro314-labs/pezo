use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use walkdir::WalkDir;

use crate::classify::{Classification, classify};
use crate::config::LibraryPaths;
use crate::db::Db;
use crate::error::{Result, io_err};
use crate::model::Candidate;
use crate::naming::humanize;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScanOutcome {
    pub discovered: Vec<Candidate>,
    pub ignored: usize,
    pub pruned: usize,
}

/// Scan the library for installer candidates: the top-level entries of the
/// library root itself (skipping the app-managed dirs), `<root>/installers`,
/// and every configured extra scan dir, then sync the candidates table with
/// what's on disk. Extra dirs that don't exist (e.g. an unplugged disk) are
/// skipped without pruning, so their candidates survive until the disk is back.
pub fn scan_library(db: &Db, paths: &LibraryPaths, scan_dirs: &[PathBuf]) -> Result<ScanOutcome> {
    let mut skip: Vec<&Path> = vec![
        paths.installers.as_path(),
        paths.games.as_path(),
        paths.prefixes.as_path(),
        paths.db_file.as_path(),
    ];
    // A scan dir nested at the top level of the root would otherwise be
    // classified as one big candidate folder and scanned inside.
    skip.extend(scan_dirs.iter().map(PathBuf::as_path));

    let mut discovered = Vec::new();
    let mut seen = Vec::new();
    let mut ignored = 0usize;
    let mut offline = Vec::new();

    scan_entries(db, &paths.root, &skip, &mut discovered, &mut seen, &mut ignored)?;
    if paths.installers.is_dir() {
        scan_entries(db, &paths.installers, &[], &mut discovered, &mut seen, &mut ignored)?;
    }
    for dir in scan_dirs {
        if dir.is_dir() {
            scan_entries(db, dir, &[], &mut discovered, &mut seen, &mut ignored)?;
        } else {
            offline.push(dir);
        }
    }

    // Prune per scope with the combined seen list; nested scopes stay correct
    // because a candidate seen anywhere is never pruned.
    let mut pruned = db.prune_candidates(&paths.root, &seen)?;
    for dir in scan_dirs {
        if dir.is_dir() {
            pruned += db.prune_candidates(dir, &seen)?;
        }
    }

    tracing::info!(
        discovered = discovered.len(),
        ignored,
        pruned,
        offline = offline.len(),
        root = %paths.root.display(),
        "scan complete"
    );

    Ok(ScanOutcome {
        discovered,
        ignored,
        pruned,
    })
}

/// Scan the top-level entries of one explicit directory and sync the
/// candidates recorded under it.
pub fn scan_dir(db: &Db, dir: &Path) -> Result<ScanOutcome> {
    let mut discovered = Vec::new();
    let mut seen = Vec::new();
    let mut ignored = 0usize;

    scan_entries(db, dir, &[], &mut discovered, &mut seen, &mut ignored)?;

    let pruned = db.prune_candidates(dir, &seen)?;
    tracing::info!(
        discovered = discovered.len(),
        ignored,
        pruned,
        dir = %dir.display(),
        "scan complete"
    );

    Ok(ScanOutcome {
        discovered,
        ignored,
        pruned,
    })
}

/// Classify the top-level entries of `dir`, upserting candidates as they are
/// found. Entries in `skip` are passed over without counting as ignored.
fn scan_entries(
    db: &Db,
    dir: &Path,
    skip: &[&Path],
    discovered: &mut Vec<Candidate>,
    seen: &mut Vec<PathBuf>,
    ignored: &mut usize,
) -> Result<()> {
    let entries = fs::read_dir(dir).map_err(io_err(dir))?;

    for entry in entries {
        let entry = entry.map_err(io_err(dir))?;
        let path = entry.path();
        if skip.iter().any(|s| *s == path) {
            continue;
        }
        match classify(&path)? {
            Classification::Ignore => {
                *ignored += 1;
            }
            Classification::Candidate(kind) => {
                let display_name = humanize(&path);
                let size = item_size(&path);
                let id = db.upsert_candidate(&path, kind, &display_name, size)?;
                seen.push(path.clone());
                discovered.push(db.get_candidate(id)?);
            }
        }
    }
    Ok(())
}

/// Size of a file, or the recursive size of a directory (best effort).
fn item_size(path: &Path) -> u64 {
    if path.is_file() {
        return path.metadata().map(|m| m.len()).unwrap_or(0);
    }
    WalkDir::new(path)
        .follow_links(false)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
        .filter_map(|e| e.metadata().ok())
        .map(|m| m.len())
        .sum()
}

/// Multipart data files (`setup_game-1.bin`) that belong to a candidate installer.
pub fn sibling_parts(installer: &Path) -> Vec<PathBuf> {
    let Some(parent) = installer.parent() else {
        return Vec::new();
    };
    let Some(stem) = installer
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
    else {
        return Vec::new();
    };
    let Ok(entries) = fs::read_dir(parent) else {
        return Vec::new();
    };
    entries
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| {
            p.extension().is_some_and(|e| e.eq_ignore_ascii_case("bin"))
                && p.file_name()
                    .map(|n| n.to_string_lossy().starts_with(&stem))
                    .unwrap_or(false)
        })
        .collect()
}
