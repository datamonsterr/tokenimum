#![allow(clippy::unwrap_used, clippy::expect_used)]
use rusqlite::Connection;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tkm_core::model::{RepoKey, Timestamp};
use tkm_store::{Repo, Store};

fn repo() -> Repo {
    Repo {
        repo_key: RepoKey("example/repo".into()),
        display_name: "example".into(),
        first_seen: Timestamp(10),
        last_seen: Timestamp(20),
        delegated_runs: 2,
    }
}

#[test]
fn roundtrip_repos() {
    let store = Store::open_in_memory().unwrap();
    let mut value = repo();
    store.upsert_repo(&value).unwrap();
    assert_eq!(
        store.get_repo(&value.repo_key).unwrap(),
        Some(value.clone())
    );
    assert_eq!(store.list_repos().unwrap(), vec![value.clone()]);
    store.increment_delegated_runs(&value.repo_key).unwrap();
    value.delegated_runs += 1;
    assert_eq!(store.get_repo(&value.repo_key).unwrap(), Some(value));
}

static COUNTER: AtomicU64 = AtomicU64::new(0);
#[test]
fn writing_under_exclusive_lock_reports_locked() {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!(
        "tkm-store-lock-{}-{nanos}-{}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("db.sqlite");
    let store = Store::open_with_timeout(&path, Duration::from_millis(1)).unwrap();
    let lock = Connection::open(&path).unwrap();
    lock.execute_batch("BEGIN EXCLUSIVE").unwrap();
    assert!(
        store
            .upsert_repo(&repo())
            .is_err_and(|error| error.is_locked())
    );
    drop(lock);
    drop(store);
    std::fs::remove_dir_all(dir).unwrap();
}
