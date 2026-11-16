//! The accounts table. One SQLite file (`accounts.db` in the app-data
//! dir), one row per signed-in account — metadata only. The cookie
//! string itself never lands here; it lives in the secret store (see
//! [`crate::secrets`]).
//!
//! Schema:
//! - `accounts(id TEXT PRIMARY KEY, data_sync_id TEXT,
//!   identity_json TEXT, visitor_data TEXT, added_at INTEGER)` —
//!   `identity_json` is NULL until `validate` fills it (blocked on
//!   the account-menu capture, `docs/missing-captures.md` §1).
//! - `settings(key TEXT PRIMARY KEY, value TEXT NOT NULL)` — holds
//!   `active_account` (the id of the active account, when any).

use std::path::Path;

use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

use crate::Error;

/// One stored account (metadata only — no cookie).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Account {
    /// `ga-<hex md5 of the SAPISID>`.
    pub id: String,
    /// Delegated identity for `context.user.onBehalfOfUser`, when known.
    pub data_sync_id: Option<String>,
    /// The validated identity (`AccountInfo` as JSON), when known.
    /// NULL until `validate` runs (blocked, see the module docs).
    pub identity_json: Option<String>,
    /// `visitorData` for `context.client`, when known.
    pub visitor_data: Option<String>,
    /// Unix seconds when the account was added.
    pub added_at: i64,
}

impl Account {
    fn from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            id: row.get(0)?,
            data_sync_id: row.get(1)?,
            identity_json: row.get(2)?,
            visitor_data: row.get(3)?,
            added_at: row.get(4)?,
        })
    }
}

/// The setting key that holds the active account's id.
const ACTIVE_ACCOUNT_KEY: &str = "active_account";

/// The accounts store. `Connection` is not `Sync`, so the app holds
/// this behind a `Mutex` (same pattern as the scaffold's `Store`).
pub struct AccountStore {
    conn: Connection,
}

impl AccountStore {
    const SCHEMA: &str = "CREATE TABLE IF NOT EXISTS accounts (
        id TEXT PRIMARY KEY,
        data_sync_id TEXT,
        identity_json TEXT,
        visitor_data TEXT,
        added_at INTEGER NOT NULL
    );
    CREATE TABLE IF NOT EXISTS settings (
        key TEXT PRIMARY KEY,
        value TEXT NOT NULL
    )";

    fn open_conn(conn: Connection) -> Result<Self, Error> {
        // The first version of this crate stored the cookie in the
        // accounts table. That schema must not survive: drop it and
        // recreate (there can be no real data in it — the popup never
        // ran against it, and cookies must not live in SQLite).
        let old_schema: bool = conn
            .query_row(
                "SELECT COUNT(*) FROM pragma_table_info('accounts') WHERE name = 'cookie'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .unwrap_or(0)
            > 0;
        if old_schema {
            tracing::warn!("dropping the old accounts table (it stored cookies in SQLite)");
            conn.execute("DROP TABLE accounts", [])?;
        }
        conn.execute_batch(Self::SCHEMA)?;
        Ok(Self { conn })
    }

    /// Open (creating) the store at `path`.
    pub fn open(path: &Path) -> Result<Self, Error> {
        Self::open_conn(Connection::open(path)?)
    }

    /// An in-memory store, for tests.
    pub fn open_in_memory() -> Result<Self, Error> {
        Self::open_conn(Connection::open_in_memory()?)
    }

    /// Insert (or replace) an account row.
    pub fn insert(&mut self, account: &Account) -> Result<(), Error> {
        self.conn.execute(
            "INSERT INTO accounts (id, data_sync_id, identity_json, visitor_data, added_at)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(id) DO UPDATE SET
                data_sync_id = excluded.data_sync_id,
                identity_json = excluded.identity_json,
                visitor_data = excluded.visitor_data,
                added_at = excluded.added_at",
            params![
                account.id,
                account.data_sync_id,
                account.identity_json,
                account.visitor_data,
                account.added_at,
            ],
        )?;
        Ok(())
    }

    /// The account with `id`, if stored.
    pub fn get(&self, id: &str) -> Result<Option<Account>, Error> {
        let mut stmt = self.conn.prepare(
            "SELECT id, data_sync_id, identity_json, visitor_data, added_at
             FROM accounts WHERE id = ?1",
        )?;
        let mut rows = stmt.query(params![id])?;
        match rows.next()? {
            Some(row) => Ok(Some(Account::from_row(row)?)),
            None => Ok(None),
        }
    }

    /// Every stored account, oldest first.
    pub fn list(&self) -> Result<Vec<Account>, Error> {
        let mut stmt = self.conn.prepare(
            "SELECT id, data_sync_id, identity_json, visitor_data, added_at
             FROM accounts ORDER BY added_at ASC",
        )?;
        let rows = stmt.query_map([], Account::from_row)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Error::from)
    }

    /// Delete an account row.
    pub fn remove(&mut self, id: &str) -> Result<usize, Error> {
        Ok(self
            .conn
            .execute("DELETE FROM accounts WHERE id = ?1", params![id])?)
    }

    /// The active account's id, if one is set.
    pub fn active_id(&self) -> Result<Option<String>, Error> {
        let mut stmt = self
            .conn
            .prepare("SELECT value FROM settings WHERE key = ?1")?;
        let mut rows = stmt.query(params![ACTIVE_ACCOUNT_KEY])?;
        match rows.next()? {
            Some(row) => Ok(Some(row.get(0)?)),
            None => Ok(None),
        }
    }

    /// Make `id` the active account. Fails with
    /// [`Error::UnknownAccount`] when the id is not stored.
    pub fn set_active(&mut self, id: &str) -> Result<(), Error> {
        if self.get(id)?.is_none() {
            return Err(Error::UnknownAccount(id.to_string()));
        }
        self.conn.execute(
            "INSERT INTO settings (key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![ACTIVE_ACCOUNT_KEY, id],
        )?;
        Ok(())
    }

    /// No account is active.
    pub fn clear_active(&mut self) -> Result<(), Error> {
        self.conn.execute(
            "DELETE FROM settings WHERE key = ?1",
            params![ACTIVE_ACCOUNT_KEY],
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn account(id: &str) -> Account {
        Account {
            id: id.to_string(),
            data_sync_id: None,
            identity_json: None,
            visitor_data: Some("visitor".to_string()),
            added_at: 1000,
        }
    }

    #[test]
    fn insert_get_list_remove_and_active_round_trip() {
        let mut store = AccountStore::open_in_memory().unwrap();
        assert!(store.active_id().unwrap().is_none());
        assert!(store.list().unwrap().is_empty());

        store.insert(&account("ga-aaa")).unwrap();
        store.insert(&account("ga-bbb")).unwrap();
        assert_eq!(store.list().unwrap().len(), 2);
        assert_eq!(store.get("ga-aaa").unwrap().unwrap().id, "ga-aaa");
        assert!(store.get("ga-zzz").unwrap().is_none());

        store.set_active("ga-bbb").unwrap();
        assert_eq!(store.active_id().unwrap().as_deref(), Some("ga-bbb"));

        store.remove("ga-bbb").unwrap();
        assert_eq!(store.list().unwrap().len(), 1);
        assert!(store.get("ga-bbb").unwrap().is_none());

        store.clear_active().unwrap();
        assert!(store.active_id().unwrap().is_none());
    }

    #[test]
    fn set_active_refuses_an_unknown_id() {
        let mut store = AccountStore::open_in_memory().unwrap();
        assert!(matches!(
            store.set_active("ga-nope"),
            Err(Error::UnknownAccount(_))
        ));
    }

    #[test]
    fn old_cookie_schema_is_dropped_on_open() {
        let dir =
            std::env::temp_dir().join(format!("tubelite-accounts-migrate-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("accounts.db");
        {
            let conn = Connection::open(&path).unwrap();
            conn.execute_batch(
                "CREATE TABLE accounts (
                    id TEXT PRIMARY KEY, label TEXT, cookie TEXT,
                    visitor_data TEXT, data_sync_id TEXT,
                    created_at INTEGER, last_used_at INTEGER, is_active INTEGER
                )",
            )
            .unwrap();
            conn.execute(
                "INSERT INTO accounts VALUES ('acc-1', 'x', 'SAPISID=old', NULL, NULL, 0, 0, 1)",
                [],
            )
            .unwrap();
        }
        let store = AccountStore::open(&path).unwrap();
        // The old row is gone, and the new schema has no cookie column.
        assert!(store.list().unwrap().is_empty());
        let cols: i64 = store
            .conn
            .query_row(
                "SELECT COUNT(*) FROM pragma_table_info('accounts') WHERE name = 'cookie'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(cols, 0);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
