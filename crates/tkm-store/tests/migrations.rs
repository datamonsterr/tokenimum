#![forbid(unsafe_code)]
#![allow(clippy::unwrap_used, clippy::expect_used)]
use rusqlite::Connection;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tkm_store::{Store, StoreError, open_read_only};

static COUNTER: AtomicU64 = AtomicU64::new(0);
struct TempDir(PathBuf);
impl TempDir {
    fn new() -> Self {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "tkm-store-{}-{nanos}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
    fn db(&self) -> PathBuf {
        self.0.join("db.sqlite")
    }
}
impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn migration_reopen_is_noop_and_preserves_version() {
    let temp = TempDir::new();
    let path = temp.db();
    drop(Store::open(&path).unwrap());
    let conn = Connection::open(&path).unwrap();
    let before: i64 = conn
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .unwrap();
    assert!(before > 0);
    drop(conn);
    drop(Store::open(&path).unwrap());
    let conn = Connection::open(&path).unwrap();
    let after: i64 = conn
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .unwrap();
    assert_eq!(before, after);
}

#[test]
fn migration_rejects_newer_database() {
    let temp = TempDir::new();
    let path = temp.db();
    let conn = Connection::open(&path).unwrap();
    conn.pragma_update(None, "user_version", 999_999).unwrap();
    drop(conn);
    assert!(matches!(
        Store::open(&path),
        Err(StoreError::NewerSchema { found: 999_999, .. })
    ));
}

#[test]
fn read_only_refuses_writes_and_missing_files() {
    let temp = TempDir::new();
    let missing = temp.0.join("missing.sqlite");
    assert!(open_read_only(&missing).is_err());
    assert!(!missing.exists());
    let path = temp.db();
    drop(Store::open(&path).unwrap());
    let conn = open_read_only(&path).unwrap();
    assert!(
        conn.execute("CREATE TABLE forbidden (id INTEGER)", [])
            .is_err()
    );
}

#[test]
fn schema_snapshot_includes_all_tables() {
    let temp = TempDir::new();
    let path = temp.db();
    drop(Store::open(&path).unwrap());
    let conn = Connection::open(path).unwrap();
    let mut statement = conn
        .prepare("SELECT type, name, sql FROM sqlite_master WHERE type = 'table' ORDER BY name")
        .unwrap();
    let rows = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    let names: Vec<_> = rows.iter().map(|(_, name, _)| name.as_str()).collect();
    assert_eq!(
        names,
        [
            "calibration",
            "decisions",
            "denylist_applied",
            "envelopes",
            "events",
            "facts",
            "installs",
            "ledger",
            "repos",
            "sessions",
            "shadow_judgments",
            "skill_evidence",
            "skill_evidence_sessions",
            "spend_attempts",
            "spend_migration",
            "tool_evidence",
            "turns"
        ]
    );
    // One type/name header and the exact SQLite CREATE text per table, separated by blank lines.
    let actual = rows
        .iter()
        .map(|(kind, name, sql)| format!("-- {kind} {name}\n{sql};\n"))
        .collect::<Vec<_>>()
        .join("\n");
    assert_eq!(actual, include_str!("snapshots/schema.sql"));
}

#[test]
fn opening_current_database_does_not_reserve_writer() {
    let temp = TempDir::new();
    let path = temp.db();
    drop(Store::open(&path).unwrap());
    let conn = Connection::open(&path).unwrap();
    conn.execute_batch("BEGIN EXCLUSIVE").unwrap();
    let result = Store::open_with_timeout(&path, Duration::from_millis(1));
    assert!(result.is_ok());
}
