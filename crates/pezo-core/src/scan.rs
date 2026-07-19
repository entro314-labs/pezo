use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use walkdir::WalkDir;

use crate::classify::{Classification, classify};
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

/// Scan the top-level entries of `dir` (normally `<library>/installers`),
/// classify each one, and sync the candidates table with what's on disk.
pub fn scan_dir(db: &Db, dir: &Path) -> Result<ScanOutcome> {
    let entries = fs::read_dir(dir).map_err(io_err(dir))?;

    let mut discovered = Vec::new();
    let mut seen = Vec::new();
    let mut ignored = 0usize;

    for entry in entries {
        let entry = entry.map_err(io_err(dir))?;
        let path = entry.path();
        match classify(&path)? {
            Classification::Ignore => {
                ignored += 1;
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
