//! SQLite store. One file, WAL mode, `synchronous=NORMAL`.
//! Versioned migrations via `PRAGMA user_version`, forward-only.
//!
//! Blocking work must never run on async runtime threads: the app crate
//! wraps this store in `spawn_blocking` / a dedicated writer thread.
//! A write that fails MUST surface an error, never report success.

use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::path::Path;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum StoreError {
    #[error("sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("migration error: {0}")]
    Migration(String),
}

pub type Result<T> = std::result::Result<T, StoreError>;

pub const SCHEMA_VERSION: u32 = 2;

const MIGRATION_V1: &str = r#"
CREATE TABLE IF NOT EXISTS settings (key TEXT PRIMARY KEY, value TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS subscriptions (channel_id TEXT PRIMARY KEY, title TEXT NOT NULL, added_at INTEGER NOT NULL);
CREATE TABLE IF NOT EXISTS history (video_id TEXT PRIMARY KEY, title TEXT NOT NULL, channel TEXT NOT NULL, played_at INTEGER NOT NULL, position_secs REAL NOT NULL DEFAULT 0, duration_secs REAL);
CREATE TABLE IF NOT EXISTS watch_later (video_id TEXT PRIMARY KEY, added_at INTEGER NOT NULL);
CREATE TABLE IF NOT EXISTS local_playlists (id TEXT PRIMARY KEY, title TEXT NOT NULL, created_at INTEGER NOT NULL);
CREATE TABLE IF NOT EXISTS playlist_items (playlist_id TEXT NOT NULL, video_id TEXT NOT NULL, position INTEGER NOT NULL, PRIMARY KEY (playlist_id, video_id));
CREATE TABLE IF NOT EXISTS resume_positions (video_id TEXT PRIMARY KEY, position_secs REAL NOT NULL, updated_at INTEGER NOT NULL);
CREATE TABLE IF NOT EXISTS metadata_cache (key TEXT PRIMARY KEY, payload TEXT NOT NULL, fetched_at INTEGER NOT NULL, ttl_secs INTEGER NOT NULL);
CREATE TABLE IF NOT EXISTS captions_cache (video_id TEXT NOT NULL, lang TEXT NOT NULL, captions_json TEXT NOT NULL, fetched_at INTEGER NOT NULL, PRIMARY KEY (video_id, lang));
"#;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Subscription {
    pub channel_id: String,
    pub title: String,
    pub added_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WatchLaterEntry {
    pub video_id: String,
    pub title: String,
    pub channel: String,
    pub added_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryEntry {
    pub video_id: String,
    pub title: String,
    pub channel: String,
    pub played_at: i64,
    pub position_secs: f64,
    pub duration_secs: Option<f64>,
}

pub struct Store {
    conn: Connection,
}

impl Store {
    pub fn open(path: &Path) -> Result<Self> {
        let conn = Connection::open(path)?;
        conn.execute_batch(
            "PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL; PRAGMA foreign_keys=ON;",
        )?;
        let mut store = Self { conn };
        store.migrate()?;
        Ok(store)
    }

    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        let mut store = Self { conn };
        store.migrate()?;
        Ok(store)
    }

    fn migrate(&mut self) -> Result<()> {
        let version: u32 = self
            .conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))?;
        if version < 1 {
            self.conn.execute_batch(MIGRATION_V1)?;
            self.conn.execute_batch("PRAGMA user_version = 1;")?;
        }
        if version < 2 {
            // v2: watch_later stores the title/channel at save time so
            // the Library page can list it without re-resolving ids.
            // Fresh v1 databases already have the table without these
            // columns; ALTER is safe because the columns have defaults.
            self.conn.execute_batch(
                "ALTER TABLE watch_later ADD COLUMN title TEXT NOT NULL DEFAULT ''; \
                 ALTER TABLE watch_later ADD COLUMN channel TEXT NOT NULL DEFAULT '';",
            )?;
            self.conn.execute_batch("PRAGMA user_version = 2;")?;
        }
        Ok(())
    }

    pub fn schema_version(&self) -> Result<u32> {
        Ok(self
            .conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))?)
    }

    pub fn set_setting(&self, key: &str, value: &str) -> Result<()> {
        self.conn.execute(
            "INSERT INTO settings (key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )?;
        Ok(())
    }

    pub fn get_setting(&self, key: &str) -> Result<Option<String>> {
        let mut stmt = self
            .conn
            .prepare("SELECT value FROM settings WHERE key = ?1")?;
        let mut rows = stmt.query(params![key])?;
        Ok(match rows.next()? {
            Some(row) => Some(row.get(0)?),
            None => None,
        })
    }

    /// Record / update a resume position. Returns the number of rows changed
    /// so callers can detect a silent no-op.
    pub fn save_resume(&self, video_id: &str, position_secs: f64, now_unix: i64) -> Result<usize> {
        Ok(self.conn.execute(
            "INSERT INTO resume_positions (video_id, position_secs, updated_at) VALUES (?1, ?2, ?3) ON CONFLICT(video_id) DO UPDATE SET position_secs = excluded.position_secs, updated_at = excluded.updated_at",
            params![video_id, position_secs, now_unix],
        )?)
    }

    pub fn get_resume(&self, video_id: &str) -> Result<Option<f64>> {
        let mut stmt = self
            .conn
            .prepare("SELECT position_secs FROM resume_positions WHERE video_id = ?1")?;
        let mut rows = stmt.query(params![video_id])?;
        Ok(match rows.next()? {
            Some(row) => Some(row.get(0)?),
            None => None,
        })
    }

    pub fn add_subscription(&self, channel_id: &str, title: &str, now_unix: i64) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO subscriptions (channel_id, title, added_at) VALUES (?1, ?2, ?3)",
            params![channel_id, title, now_unix],
        )?;
        Ok(())
    }

    pub fn remove_subscription(&self, channel_id: &str) -> Result<usize> {
        Ok(self.conn.execute(
            "DELETE FROM subscriptions WHERE channel_id = ?1",
            params![channel_id],
        )?)
    }

    pub fn list_subscriptions(&self) -> Result<Vec<Subscription>> {
        let mut stmt = self.conn.prepare(
            "SELECT channel_id, title, added_at FROM subscriptions ORDER BY title COLLATE NOCASE",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(Subscription {
                channel_id: row.get(0)?,
                title: row.get(1)?,
                added_at: row.get(2)?,
            })
        })?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Into::into)
    }

    pub fn list_history(&self, limit: u32) -> Result<Vec<HistoryEntry>> {
        let mut stmt = self.conn.prepare(
            "SELECT video_id, title, channel, played_at, position_secs, duration_secs FROM history ORDER BY played_at DESC LIMIT ?1",
        )?;
        let rows = stmt.query_map(params![limit], |row| {
            Ok(HistoryEntry {
                video_id: row.get(0)?,
                title: row.get(1)?,
                channel: row.get(2)?,
                played_at: row.get(3)?,
                position_secs: row.get(4)?,
                duration_secs: row.get(5)?,
            })
        })?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Into::into)
    }

    pub fn add_watch_later(
        &self,
        video_id: &str,
        title: &str,
        channel: &str,
        now_unix: i64,
    ) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO watch_later (video_id, title, channel, added_at) VALUES (?1, ?2, ?3, ?4)",
            params![video_id, title, channel, now_unix],
        )?;
        Ok(())
    }

    pub fn remove_watch_later(&self, video_id: &str) -> Result<usize> {
        Ok(self.conn.execute(
            "DELETE FROM watch_later WHERE video_id = ?1",
            params![video_id],
        )?)
    }

    pub fn list_watch_later(&self) -> Result<Vec<WatchLaterEntry>> {
        let mut stmt = self.conn.prepare(
            "SELECT video_id, title, channel, added_at FROM watch_later ORDER BY added_at DESC",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(WatchLaterEntry {
                video_id: row.get(0)?,
                title: row.get(1)?,
                channel: row.get(2)?,
                added_at: row.get(3)?,
            })
        })?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Into::into)
    }

    pub fn record_history(
        &self,
        video_id: &str,
        title: &str,
        channel: &str,
        played_at: i64,
        position_secs: f64,
        duration_secs: Option<f64>,
    ) -> Result<()> {
        self.conn.execute(
            "INSERT INTO history (video_id, title, channel, played_at, position_secs, duration_secs) VALUES (?1, ?2, ?3, ?4, ?5, ?6) ON CONFLICT(video_id) DO UPDATE SET title = excluded.title, played_at = excluded.played_at, position_secs = excluded.position_secs, duration_secs = excluded.duration_secs",
            params![video_id, title, channel, played_at, position_secs, duration_secs],
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn migrations_run_and_settings_roundtrip() {
        let store = Store::open_in_memory().unwrap();
        assert_eq!(store.schema_version().unwrap(), SCHEMA_VERSION);
        store.set_setting("theme", "dark").unwrap();
        assert_eq!(store.get_setting("theme").unwrap().as_deref(), Some("dark"));
        assert_eq!(store.get_setting("missing").unwrap(), None);
    }

    #[test]
    fn subscriptions_roundtrip() {
        let store = Store::open_in_memory().unwrap();
        assert!(store.list_subscriptions().unwrap().is_empty());
        store.add_subscription("UCaaa", "Channel A", 1000).unwrap();
        store.add_subscription("UCbbb", "Channel B", 1001).unwrap();
        let subs = store.list_subscriptions().unwrap();
        assert_eq!(subs.len(), 2);
        assert_eq!(subs[0].title, "Channel A"); // ordered by title
        assert_eq!(store.remove_subscription("UCaaa").unwrap(), 1);
        assert_eq!(store.remove_subscription("UCaaa").unwrap(), 0);
        assert_eq!(store.list_subscriptions().unwrap().len(), 1);
    }

    #[test]
    fn watch_later_roundtrip() {
        let store = Store::open_in_memory().unwrap();
        assert_eq!(store.schema_version().unwrap(), 2);
        assert!(store.list_watch_later().unwrap().is_empty());
        store
            .add_watch_later("vid1", "First", "Ch A", 1000)
            .unwrap();
        store
            .add_watch_later("vid2", "Second", "Ch B", 2000)
            .unwrap();
        let list = store.list_watch_later().unwrap();
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].video_id, "vid2"); // newest first
        assert_eq!(list[1].title, "First");
        assert_eq!(store.remove_watch_later("vid1").unwrap(), 1);
        assert_eq!(store.remove_watch_later("vid1").unwrap(), 0);
        assert_eq!(store.list_watch_later().unwrap().len(), 1);
    }

    #[test]
    fn history_lists_newest_first() {
        let store = Store::open_in_memory().unwrap();
        store
            .record_history("vid1", "First", "Ch A", 1000, 10.0, Some(100.0))
            .unwrap();
        store
            .record_history("vid2", "Second", "Ch B", 2000, 0.0, None)
            .unwrap();
        let h = store.list_history(10).unwrap();
        assert_eq!(h.len(), 2);
        assert_eq!(h[0].video_id, "vid2"); // newest first
        assert_eq!(h[1].position_secs, 10.0);
        assert_eq!(store.list_history(1).unwrap().len(), 1);
    }

    #[test]
    fn resume_position_roundtrip_and_update() {
        let store = Store::open_in_memory().unwrap();
        assert_eq!(store.get_resume("v1").unwrap(), None);
        let changed = store.save_resume("v1", 42.5, 1_700_000_000).unwrap();
        assert_eq!(changed, 1);
        assert_eq!(store.get_resume("v1").unwrap(), Some(42.5));
        store.save_resume("v1", 90.0, 1_700_000_100).unwrap();
        assert_eq!(store.get_resume("v1").unwrap(), Some(90.0));
    }
}
