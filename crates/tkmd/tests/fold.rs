//! E4.5 fold workers.
#![forbid(unsafe_code)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::pedantic)]

mod common;

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use common::*;
use tkm_proto::{EventKind, Host};
use tkmd::clock::ManualClock;
use tkmd::daemon::{Daemon, DaemonOptions};
use tkmd::decider::NullDecider;
use tkmd::settings::Settings;

fn transcript() -> String {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/claude/sessions/e928a2cf-e31b-4154-805b-919991d042ea.jsonl")
        .to_string_lossy()
        .into_owned()
}

fn end_request(transcript_path: &str) -> tkm_proto::Request {
    let mut payload = fixture("SessionEnd");
    payload["transcript_path"] = serde_json::json!(transcript_path);
    request(EventKind::SessionEnd, payload)
}

fn start_request(transcript_path: &str) -> tkm_proto::Request {
    let mut payload = fixture("SessionStart");
    payload["transcript_path"] = serde_json::json!(transcript_path);
    request(EventKind::SessionStart, payload)
}

#[tokio::test]
async fn session_end_marks_the_session_and_a_detached_fold_writes_the_ledger() {
    let env = Env::new();
    let daemon = daemon_with(&env, Settings::default(), Arc::new(NullDecider));
    daemon.handle(start_request(&transcript())).await;
    let reply = daemon.handle(end_request(&transcript())).await;
    assert_eq!(
        reply.stdout, "",
        "session_end replies at once; the fold is detached"
    );
    daemon.settle().await;

    let store = tkm_store::open_read_only(&env.paths.db).unwrap();
    let (status, ended): (String, Option<i64>) = store
        .query_row("SELECT fold_status, ended_at FROM sessions", [], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })
        .unwrap();
    assert!(ended.is_some());
    assert_eq!(status, "folded");
    let rows: i64 = store
        .query_row("SELECT COUNT(*) FROM ledger", [], |r| r.get(0))
        .unwrap();
    assert_eq!(rows, 1, "ledger row written by the fold");
    assert!(env.error_lines().is_empty(), "{:?}", env.error_lines());
}

#[tokio::test]
async fn partial_session_contributes_to_the_ledger_only() {
    let env = Env::new();
    // 3 of 10 records are unreadable: more than 20% skipped.
    let dir = env.dir.path().join("logs");
    std::fs::create_dir_all(&dir).unwrap();
    let good = std::fs::read_to_string(transcript()).unwrap();
    let mut lines: Vec<&str> = good.lines().collect();
    lines.truncate(7);
    let mut text = lines.join("\n");
    text.push_str("\n{broken\n{broken\n{broken\n");
    let path = dir.join("partial.jsonl");
    std::fs::write(&path, text).unwrap();

    let daemon = daemon_with(&env, Settings::default(), Arc::new(NullDecider));
    daemon.handle(start_request(path.to_str().unwrap())).await;
    daemon.handle(end_request(path.to_str().unwrap())).await;
    daemon.settle().await;
    let store = tkm_store::open_read_only(&env.paths.db).unwrap();
    let facts: i64 = store
        .query_row("SELECT COUNT(*) FROM facts", [], |r| r.get(0))
        .unwrap();
    assert_eq!(facts, 0, "partial sessions never create facts");
    let status: String = store
        .query_row("SELECT fold_status FROM sessions", [], |r| r.get(0))
        .unwrap();
    assert!(status == "folded" || status == "partial", "{status}");
}

#[tokio::test]
async fn corrupt_session_log_logs_one_line_and_the_daemon_keeps_serving() {
    let env = Env::new();
    let path = env.dir.path().join("garbage.jsonl");
    std::fs::write(&path, [0xff_u8, 0xfe, 0x00, 0x12, b'\n', b'{']).unwrap();
    let daemon = daemon_with(&env, Settings::default(), Arc::new(NullDecider));
    daemon.handle(start_request(path.to_str().unwrap())).await;
    daemon.handle(end_request(path.to_str().unwrap())).await;
    daemon.settle().await;
    assert!(env.error_lines().len() <= 1, "{:?}", env.error_lines());
    // Still serving.
    let reply = daemon.handle(session_start()).await;
    assert_eq!(reply.stdout, "");
}

#[tokio::test]
async fn missing_log_file_is_one_errors_log_line_and_status_error() {
    let env = Env::new();
    let daemon = daemon_with(&env, Settings::default(), Arc::new(NullDecider));
    daemon
        .handle(start_request("/definitely/not/here.jsonl"))
        .await;
    daemon
        .handle(end_request("/definitely/not/here.jsonl"))
        .await;
    daemon.settle().await;
    assert_eq!(env.error_lines().len(), 1);
    let store = tkm_store::open_read_only(&env.paths.db).unwrap();
    let status: String = store
        .query_row("SELECT fold_status FROM sessions", [], |r| r.get(0))
        .unwrap();
    assert_eq!(status, "error");
}

#[tokio::test]
async fn idle_sessions_fold_after_thirty_minutes_on_a_fake_clock() {
    let env = Env::new();
    let clock = Arc::new(ManualClock::new(1_800_000_000_000));
    let mut options = DaemonOptions::new(env.paths.clone());
    options.decider = Arc::new(NullDecider);
    options.clock = clock.clone();
    options.settings = settings_fixed(Settings::default());
    options.idle_after = Duration::from_secs(30 * 60);
    let daemon = Daemon::new(options);
    daemon.handle(start_request(&transcript())).await;
    daemon.handle(prompt()).await;

    clock.advance(Duration::from_secs(29 * 60));
    daemon.scan_idle().await;
    daemon.settle().await;
    assert_eq!(fold_status(&env), None, "not idle long enough");

    clock.advance(Duration::from_secs(2 * 60));
    daemon.scan_idle().await;
    daemon.settle().await;
    assert_eq!(fold_status(&env).as_deref(), Some("folded"));
}

#[tokio::test]
async fn unfinished_earlier_session_in_the_same_repo_folds_at_the_next_session_start() {
    let env = Env::new();
    let clock = Arc::new(ManualClock::new(1_800_000_000_000));
    let mut options = DaemonOptions::new(env.paths.clone());
    options.decider = Arc::new(NullDecider);
    options.clock = clock.clone();
    options.settings = settings_fixed(Settings::default());
    let daemon = Daemon::new(options);
    daemon.handle(start_request(&transcript())).await;
    daemon.handle(prompt()).await;
    // Same repo, a different session starts later without the first ever ending.
    clock.advance(Duration::from_secs(6 * 60));
    let mut payload = fixture("SessionStart");
    payload["session_id"] = serde_json::json!("another-session");
    daemon
        .handle(request(EventKind::SessionStart, payload))
        .await;
    daemon.settle().await;
    assert_eq!(fold_status(&env).as_deref(), Some("folded"));
    let _ = Host::Claude;
}

#[tokio::test]
async fn watcher_scan_fallback_folds_idle_sessions_every_sixty_seconds() {
    let env = Env::new();
    let clock = Arc::new(ManualClock::new(1_800_000_000_000));
    let mut options = DaemonOptions::new(env.paths.clone());
    options.decider = Arc::new(NullDecider);
    options.clock = clock.clone();
    options.settings = settings_fixed(Settings::default());
    options.idle_after = Duration::from_secs(60);
    options.scan_interval = Duration::from_millis(50);
    let daemon = Daemon::new(options);
    daemon.start_workers();
    daemon.handle(start_request(&transcript())).await;
    clock.advance(Duration::from_secs(120));
    assert!(eventually(10, || fold_status(&env).as_deref() == Some("folded")).await);
}

fn fold_status(env: &Env) -> Option<String> {
    let store = tkm_store::open_read_only(&env.paths.db).ok()?;
    store
        .query_row("SELECT fold_status FROM sessions", [], |r| {
            r.get::<_, String>(0)
        })
        .ok()
        .filter(|s| s != "open")
}
