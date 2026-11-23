//! Pezo core: scan a library disk for offline game installers and folders,
//! classify them, install them (innoextract / wine prefixes / in-place), and
//! launch them through the right runner (Wine/Proton, DOSBox, ScummVM).
//!
//! The heavy lifting is delegated to battle-tested external tools; this crate
//! is the orchestration and bookkeeping layer shared by the CLI and the Tauri app.

pub mod classify;
pub mod config;
pub mod covers;
pub mod db;
pub mod deps;
pub mod desktop;
pub mod error;
pub mod install;
pub mod launch;
pub mod model;
pub mod naming;
pub mod scan;
pub mod wine;

pub use config::{Config, FsInfo, LibraryPaths};
pub use db::Db;
pub use error::{CoreError, Result};
pub use model::{Candidate, CandidateKind, Game, NewGame, Runner};

/// Open the library database for the configured library root, ensuring the
/// on-disk layout exists.
pub fn open_library(config: &Config) -> Result<(LibraryPaths, Db)> {
    let paths = config.library()?;
    paths.ensure_layout()?;
    let db = Db::open(&paths.db_file)?;
    Ok((paths, db))
}
