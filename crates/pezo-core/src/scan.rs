use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::UNIX_EPOCH;

use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use walkdir::WalkDir;

use crate::classify::{Classification, classify, multipart_parts};
use crate::config::LibraryPaths;
use crate::db::Db;
use crate::error::{Result, io_err};
use crate::model::{Candidate, CandidateKind};
use crate::naming::humanize;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScanOutcome {
    pub discovered: Vec<Candidate>,
    pub ignored: usize,
    pub pruned: usize,
    /// Configured scan dirs that were skipped because they don't exist right
    /// now (e.g. an unplugged disk). Their candidates are kept, not pruned.
    #[serde(default)]
    pub offline: Vec<PathBuf>,
}

/// Knobs for a scan run.
#[derive(Default)]
pub struct ScanOptions<'a> {
    /// Re-classify everything, ignoring stored modification stamps.
    pub full: bool,
    /// Called with (entries done, entries total) as the scan advances.
    pub progress: Option<&'a (dyn Fn(usize, usize) + Sync)>,
}

/// Scan the library for installer candidates: the top-level entries of the
/// library root itself (skipping the app-managed dirs), `<root>/installers`,
/// and every configured extra scan dir, then sync the candidates table with
/// what's on disk. Extra dirs that don't exist (e.g. an unplugged disk) are
/// skipped without pruning, so their candidates survive until the disk is back.
///
/// Rescans are incremental: entries whose modification time matches the stored
/// stamp keep their classification without touching their contents.
pub fn scan_library(
    db: &Db,
    paths: &LibraryPaths,
    scan_dirs: &[PathBuf],
    opts: &ScanOptions<'_>,
) -> Result<ScanOutcome> {
    let mut skip: Vec<PathBuf> = vec![
        paths.installers.clone(),
        paths.games.clone(),
        paths.prefixes.clone(),
        paths.db_file.clone(),
        paths.covers(),
    ];
    // A scan dir nested at the top level of the root would otherwise be
    // classified as one big candidate folder and scanned inside.
    skip.extend(scan_dirs.iter().cloned());

    let mut entries = Vec::new();
    let mut offline = Vec::new();
    collect_entries(&paths.root, &skip, &mut entries)?;
    if paths.installers.is_dir() {
        collect_entries(&paths.installers, &[], &mut entries)?;
    }
    for dir in scan_dirs {
        if dir.is_dir() {
            collect_entries(dir, &[], &mut entries)?;
        } else {
            offline.push(dir.clone());
        }
    }

    let (discovered, seen, ignored) = run_scan(db, entries, opts)?;

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
        offline,
    })
}

/// Scan the top-level entries of one explicit directory and sync the
/// candidates recorded under it.
pub fn scan_dir(db: &Db, dir: &Path, opts: &ScanOptions<'_>) -> Result<ScanOutcome> {
    let mut entries = Vec::new();
    collect_entries(dir, &[], &mut entries)?;

    let (discovered, seen, ignored) = run_scan(db, entries, opts)?;
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
        offline: Vec::new(),
    })
}

/// Top-level entries of `dir`, minus exact paths in `skip`.
fn collect_entries(dir: &Path, skip: &[PathBuf], out: &mut Vec<PathBuf>) -> Result<()> {
    for entry in fs::read_dir(dir).map_err(io_err(dir))? {
        let path = entry.map_err(io_err(dir))?.path();
        if !skip.contains(&path) {
            out.push(path);
        }
    }
    Ok(())
}

type ClassifiedEntry = (PathBuf, Option<i64>, Option<(CandidateKind, String, u64)>);

/// Classify `entries` (in parallel, skipping unchanged known candidates) and
/// upsert the results. Returns (discovered candidates, seen paths, ignored).
fn run_scan(
    db: &Db,
    entries: Vec<PathBuf>,
    opts: &ScanOptions<'_>,
) -> Result<(Vec<Candidate>, Vec<PathBuf>, usize)> {
    let total = entries.len();

    // Unchanged known candidates keep their row untouched.
    let mut cached_ids = Vec::new();
    let mut seen = Vec::new();
    let mut jobs = Vec::new();
    for path in entries {
        let mtime = entry_mtime(&path);
        if !opts.full
            && let Some(m) = mtime
            && let Some((id, Some(stored))) = db.candidate_meta(&path)?
            && m == stored
        {
            cached_ids.push(id);
            seen.push(path);
            continue;
        }
        jobs.push((path, mtime));
    }

    let done = AtomicUsize::new(cached_ids.len());
    if let Some(cb) = opts.progress {
        cb(done.load(Ordering::Relaxed), total);
    }

    // Classification reads the filesystem only — safe to parallelize. DB
    // writes stay on this thread afterwards.
    let classified: Vec<Result<ClassifiedEntry>> = jobs
        .into_par_iter()
        .map(|(path, mtime)| {
            let result = match classify(&path)? {
                Classification::Ignore => (path, mtime, None),
                Classification::Candidate(kind) => {
                    let display_name = humanize(&path);
                    let size = item_size(&path);
                    (path, mtime, Some((kind, display_name, size)))
                }
            };
            let d = done.fetch_add(1, Ordering::Relaxed) + 1;
            if let Some(cb) = opts.progress {
                cb(d, total);
            }
            Ok(result)
        })
        .collect();

    let mut discovered = Vec::new();
    let mut ignored = 0usize;
    for item in classified {
        let (path, mtime, outcome) = item?;
        match outcome {
            None => ignored += 1,
            Some((kind, display_name, size)) => {
                let id = db.upsert_candidate(&path, kind, &display_name, size, mtime)?;
                seen.push(path);
                discovered.push(db.get_candidate(id)?);
            }
        }
    }
    for id in cached_ids {
        discovered.push(db.get_candidate(id)?);
    }

    Ok((discovered, seen, ignored))
}

/// Modification time of a scan entry in unix seconds, if retrievable.
fn entry_mtime(path: &Path) -> Option<i64> {
    let modified = path.metadata().ok()?.modified().ok()?;
    Some(match modified.duration_since(UNIX_EPOCH) {
        Ok(d) => d.as_secs() as i64,
        Err(e) => -(e.duration().as_secs() as i64),
    })
}

/// Size of a file, or the recursive size of a directory (best effort).
/// A split-archive head counts the whole set, since installing it needs
/// (and consumes) every part.
fn item_size(path: &Path) -> u64 {
    if path.is_file() {
        let parts = multipart_parts(path);
        if parts.len() > 1 {
            return parts
                .iter()
                .filter_map(|p| p.metadata().ok())
                .map(|m| m.len())
                .sum();
        }
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
