use std::path::{Path, PathBuf};

use rusqlite::{Connection, OptionalExtension, params};

use crate::error::{CoreError, Result};
use crate::model::{Candidate, CandidateKind, Game, NewGame, Runner};

/// SQLite-backed library database. Lives on the library disk (`<root>/pezo.db`)
/// so the library is self-contained and portable.
pub struct Db {
    conn: Connection,
}

impl Db {
    pub fn open(path: &Path) -> Result<Self> {
        let conn = Connection::open(path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        let db = Self { conn };
        db.migrate()?;
        Ok(db)
    }

    fn migrate(&self) -> Result<()> {
        let version: i64 = self
            .conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))?;
        if version < 1 {
            self.conn.execute_batch(
                r#"
                CREATE TABLE IF NOT EXISTS games (
                    id INTEGER PRIMARY KEY,
                    slug TEXT NOT NULL UNIQUE,
                    title TEXT NOT NULL,
                    runner TEXT NOT NULL,
                    install_path TEXT NOT NULL,
                    launch_path TEXT NOT NULL,
                    prefix_path TEXT,
                    source_path TEXT,
                    added_at TEXT NOT NULL DEFAULT (datetime('now')),
                    last_played_at TEXT,
                    play_seconds INTEGER NOT NULL DEFAULT 0
                );

                CREATE TABLE IF NOT EXISTS candidates (
                    id INTEGER PRIMARY KEY,
                    path TEXT NOT NULL UNIQUE,
                    kind TEXT NOT NULL,
                    display_name TEXT NOT NULL,
                    size_bytes INTEGER NOT NULL DEFAULT 0,
                    discovered_at TEXT NOT NULL DEFAULT (datetime('now')),
                    installed_game_id INTEGER REFERENCES games(id) ON DELETE SET NULL
                );

                CREATE INDEX IF NOT EXISTS idx_candidates_game ON candidates(installed_game_id);

                PRAGMA user_version = 1;
                "#,
            )?;
        }
        let version: i64 = self
            .conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))?;
        if version < 2 {
            self.conn.execute_batch(
                r#"
                ALTER TABLE candidates ADD COLUMN mtime INTEGER;

                ALTER TABLE games ADD COLUMN store_id TEXT;
                ALTER TABLE games ADD COLUMN store TEXT;
                ALTER TABLE games ADD COLUMN launch_args TEXT NOT NULL DEFAULT '';
                ALTER TABLE games ADD COLUMN env_vars TEXT NOT NULL DEFAULT '';
                ALTER TABLE games ADD COLUMN wrappers TEXT NOT NULL DEFAULT '';

                PRAGMA user_version = 2;
                "#,
            )?;
        }
        Ok(())
    }

    // ---- candidates ----

    pub fn upsert_candidate(
        &self,
        path: &Path,
        kind: CandidateKind,
        display_name: &str,
        size_bytes: u64,
        mtime: Option<i64>,
    ) -> Result<i64> {
        let id: i64 = self.conn.query_row(
            r#"
            INSERT INTO candidates (path, kind, display_name, size_bytes, mtime)
            VALUES (?1, ?2, ?3, ?4, ?5)
            ON CONFLICT(path) DO UPDATE SET
                kind = excluded.kind,
                display_name = excluded.display_name,
                size_bytes = excluded.size_bytes,
                mtime = excluded.mtime
            RETURNING id
            "#,
            params![
                path_str(path)?,
                kind.as_str(),
                display_name,
                size_bytes as i64,
                mtime
            ],
            |r| r.get(0),
        )?;
        Ok(id)
    }

    /// Stored id + scan-time modification stamp for a path, if it's a known
    /// candidate. Drives incremental rescans.
    pub fn candidate_meta(&self, path: &Path) -> Result<Option<(i64, Option<i64>)>> {
        Ok(self
            .conn
            .query_row(
                "SELECT id, mtime FROM candidates WHERE path = ?1",
                params![path_str(path)?],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?)
    }

    /// Remove candidates under `scope` whose paths were not seen in the latest
    /// scan and which are not linked to an installed game.
    pub fn prune_candidates(&self, scope: &Path, seen: &[PathBuf]) -> Result<usize> {
        let mut stmt = self.conn.prepare(
            "SELECT id, path FROM candidates WHERE installed_game_id IS NULL AND path LIKE ?1",
        )?;
        let like = format!("{}%", path_str(scope)?);
        let rows: Vec<(i64, String)> = stmt
            .query_map(params![like], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<std::result::Result<_, _>>()?;
        let mut pruned = 0;
        for (id, path) in rows {
            if !seen
                .iter()
                .any(|p| p.as_os_str() == std::ffi::OsStr::new(&path))
            {
                self.conn
                    .execute("DELETE FROM candidates WHERE id = ?1", params![id])?;
                pruned += 1;
            }
        }
        Ok(pruned)
    }

    pub fn list_candidates(&self) -> Result<Vec<Candidate>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, path, kind, display_name, size_bytes, discovered_at, installed_game_id
             FROM candidates ORDER BY display_name COLLATE NOCASE",
        )?;
        let rows = stmt.query_map([], row_to_candidate)?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    pub fn get_candidate(&self, id: i64) -> Result<Candidate> {
        self.conn
            .query_row(
                "SELECT id, path, kind, display_name, size_bytes, discovered_at, installed_game_id
                 FROM candidates WHERE id = ?1",
                params![id],
                row_to_candidate,
            )
            .optional()?
            .ok_or_else(|| CoreError::NotFound(format!("candidate {id}")))
    }

    pub fn link_candidate(&self, candidate_id: i64, game_id: i64) -> Result<()> {
        self.conn.execute(
            "UPDATE candidates SET installed_game_id = ?2 WHERE id = ?1",
            params![candidate_id, game_id],
        )?;
        Ok(())
    }

    // ---- games ----

    pub fn insert_game(&self, new: &NewGame<'_>) -> Result<Game> {
        let id: i64 = self.conn.query_row(
            r#"
            INSERT INTO games (slug, title, runner, install_path, launch_path,
                               prefix_path, source_path, store_id, store)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
            RETURNING id
            "#,
            params![
                new.slug,
                new.title,
                new.runner.as_str(),
                path_str(new.install_path)?,
                path_str(new.launch_path)?,
                new.prefix_path.map(path_str).transpose()?,
                new.source_path.map(path_str).transpose()?,
                new.store_id,
                new.store,
            ],
            |r| r.get(0),
        )?;
        self.get_game(id)
    }

    /// Update per-game launch customization (args, env lines, wrapper list).
    pub fn update_launch_options(
        &self,
        game_id: i64,
        launch_args: &str,
        env_vars: &str,
        wrappers: &str,
    ) -> Result<()> {
        let n = self.conn.execute(
            "UPDATE games SET launch_args = ?2, env_vars = ?3, wrappers = ?4 WHERE id = ?1",
            params![game_id, launch_args, env_vars, wrappers],
        )?;
        if n == 0 {
            return Err(CoreError::NotFound(format!("game {game_id}")));
        }
        Ok(())
    }

    pub fn list_games(&self) -> Result<Vec<Game>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, slug, title, runner, install_path, launch_path, prefix_path,
                    source_path, added_at, last_played_at, play_seconds,
                    store_id, store, launch_args, env_vars, wrappers
             FROM games ORDER BY title COLLATE NOCASE",
        )?;
        let rows = stmt.query_map([], row_to_game)?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    pub fn get_game(&self, id: i64) -> Result<Game> {
        self.conn
            .query_row(
                "SELECT id, slug, title, runner, install_path, launch_path, prefix_path,
                        source_path, added_at, last_played_at, play_seconds,
                        store_id, store, launch_args, env_vars, wrappers
                 FROM games WHERE id = ?1",
                params![id],
                row_to_game,
            )
            .optional()?
            .ok_or_else(|| CoreError::NotFound(format!("game {id}")))
    }

    pub fn find_game_by_slug(&self, slug: &str) -> Result<Option<Game>> {
        Ok(self
            .conn
            .query_row(
                "SELECT id, slug, title, runner, install_path, launch_path, prefix_path,
                        source_path, added_at, last_played_at, play_seconds,
                        store_id, store, launch_args, env_vars, wrappers
                 FROM games WHERE slug = ?1",
                params![slug],
                row_to_game,
            )
            .optional()?)
    }

    pub fn slug_taken(&self, slug: &str) -> Result<bool> {
        let n: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM games WHERE slug = ?1",
            params![slug],
            |r| r.get(0),
        )?;
        Ok(n > 0)
    }

    pub fn record_play(&self, game_id: i64, seconds: i64) -> Result<()> {
        self.conn.execute(
            "UPDATE games SET last_played_at = datetime('now'),
                              play_seconds = play_seconds + ?2
             WHERE id = ?1",
            params![game_id, seconds],
        )?;
        Ok(())
    }

    pub fn remove_game(&self, game_id: i64) -> Result<()> {
        self.conn.execute(
            "UPDATE candidates SET installed_game_id = NULL WHERE installed_game_id = ?1",
            params![game_id],
        )?;
        let n = self
            .conn
            .execute("DELETE FROM games WHERE id = ?1", params![game_id])?;
        if n == 0 {
            return Err(CoreError::NotFound(format!("game {game_id}")));
        }
        Ok(())
    }
}

fn path_str(path: &Path) -> Result<String> {
    path.to_str().map(str::to_owned).ok_or_else(|| {
        CoreError::InvalidState(format!("path is not valid UTF-8: {}", path.display()))
    })
}

fn row_to_candidate(r: &rusqlite::Row<'_>) -> rusqlite::Result<Candidate> {
    let kind_raw: String = r.get(2)?;
    Ok(Candidate {
        id: r.get(0)?,
        path: PathBuf::from(r.get::<_, String>(1)?),
        kind: CandidateKind::parse(&kind_raw).map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(2, rusqlite::types::Type::Text, Box::new(e))
        })?,
        display_name: r.get(3)?,
        size_bytes: r.get::<_, i64>(4)?.max(0) as u64,
        discovered_at: r.get(5)?,
        installed_game_id: r.get(6)?,
    })
}

fn row_to_game(r: &rusqlite::Row<'_>) -> rusqlite::Result<Game> {
    let runner_raw: String = r.get(3)?;
    Ok(Game {
        id: r.get(0)?,
        slug: r.get(1)?,
        title: r.get(2)?,
        runner: Runner::parse(&runner_raw).map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(3, rusqlite::types::Type::Text, Box::new(e))
        })?,
        install_path: PathBuf::from(r.get::<_, String>(4)?),
        launch_path: PathBuf::from(r.get::<_, String>(5)?),
        prefix_path: r.get::<_, Option<String>>(6)?.map(PathBuf::from),
        source_path: r.get::<_, Option<String>>(7)?.map(PathBuf::from),
        added_at: r.get(8)?,
        last_played_at: r.get(9)?,
        play_seconds: r.get(10)?,
        store_id: r.get(11)?,
        store: r.get(12)?,
        launch_args: r.get(13)?,
        env_vars: r.get(14)?,
        wrappers: r.get(15)?,
    })
}
