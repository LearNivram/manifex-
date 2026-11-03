//! Local persistence (SQLite).
//!
//! SQLite is used for everything that stays on the user's machine: cluster
//! profiles (non-secret metadata), encrypted secrets, preferences, alert rules and
//! the local audit trail. The database lives in the platform's application data
//! directory and is opened with WAL journaling for crash safety.

pub mod audit;
pub mod clusters;
pub mod rules;
pub mod secrets;
pub mod settings;

use std::path::Path;

use parking_lot::Mutex;
use rusqlite::Connection;

use crate::error::AppResult;

const MIGRATIONS: &[&str] = &[
    // v1
    r#"
    CREATE TABLE clusters (
        id TEXT PRIMARY KEY,
        name TEXT NOT NULL,
        source TEXT NOT NULL,
        group_name TEXT,
        favorite INTEGER NOT NULL DEFAULT 0,
        color TEXT,
        environment TEXT NOT NULL DEFAULT 'development',
        default_namespace TEXT,
        alerts_enabled INTEGER NOT NULL DEFAULT 1,
        sort_order INTEGER NOT NULL DEFAULT 0,
        created_at TEXT NOT NULL,
        last_connected_at TEXT
    );
    CREATE TABLE secrets (
        id TEXT PRIMARY KEY,
        nonce BLOB NOT NULL,
        ciphertext BLOB NOT NULL,
        created_at TEXT NOT NULL
    );
    CREATE TABLE settings (
        key TEXT PRIMARY KEY,
        value TEXT NOT NULL
    );
    CREATE TABLE audit_log (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        ts TEXT NOT NULL,
        cluster_id TEXT,
        cluster_name TEXT,
        identity TEXT,
        action TEXT NOT NULL,
        resource TEXT,
        namespace TEXT,
        name TEXT,
        outcome TEXT NOT NULL,
        detail TEXT
    );
    CREATE INDEX audit_log_ts ON audit_log(ts DESC);
    CREATE INDEX audit_log_cluster ON audit_log(cluster_id);
    CREATE TABLE alert_rules (
        id TEXT PRIMARY KEY,
        name TEXT NOT NULL,
        enabled INTEGER NOT NULL DEFAULT 1,
        cluster_id TEXT,
        namespace TEXT,
        kind TEXT,
        reason_pattern TEXT,
        message_pattern TEXT,
        severity TEXT NOT NULL DEFAULT 'warning',
        created_at TEXT NOT NULL
    );
    "#,
];

pub struct Db {
    conn: Mutex<Connection>,
}

impl Db {
    pub fn open(path: &Path) -> AppResult<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(path)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
        }
        Self::init(conn)
    }

    pub fn open_in_memory() -> AppResult<Self> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> AppResult<Self> {
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        let db = Self {
            conn: Mutex::new(conn),
        };
        db.migrate()?;
        Ok(db)
    }

    fn migrate(&self) -> AppResult<()> {
        let conn = self.conn.lock();
        let version: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        for (idx, sql) in MIGRATIONS.iter().enumerate() {
            let target = idx as i64 + 1;
            if version < target {
                conn.execute_batch(&format!(
                    "BEGIN; {sql}; PRAGMA user_version = {target}; COMMIT;"
                ))?;
                tracing::info!(target, "applied database migration");
            }
        }
        Ok(())
    }

    pub fn with<T>(&self, f: impl FnOnce(&Connection) -> rusqlite::Result<T>) -> AppResult<T> {
        let conn = self.conn.lock();
        Ok(f(&conn)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrations_apply_idempotently() {
        let db = Db::open_in_memory().unwrap();
        db.migrate().unwrap();
        let v: i64 = db
            .with(|c| c.query_row("PRAGMA user_version", [], |r| r.get(0)))
            .unwrap();
        assert_eq!(v, MIGRATIONS.len() as i64);
    }
}
