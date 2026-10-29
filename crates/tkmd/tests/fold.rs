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
async fn late_resolution_does_not_reopen_an_ended_session() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Condvar, Mutex};
    use tkmd::settings::SettingsSource;

    struct DelayedSettings {
        calls: AtomicUsize,
        gate: (Mutex<bool>, Condvar),
    }
    impl SettingsSource for DelayedSettings {
        fn settings_for(&self, root: Option<&Path>) -> Arc<Settings> {
            if root.is_some() && self.calls.fetch_add(1, Ordering::SeqCst) == 0 {
                let (lock, wake) = &self.gate;
                let mut released = lock.lock().unwrap();
                while !*released {
                    released = wake.wait(released).unwrap();
                }
            }
            Arc::new(Settings::default())
        }
    }
    let env = Env::new();
    let delayed = Arc::new(DelayedSettings {
        calls: AtomicUsize::new(0),
        gate: (Mutex::new(false), Condvar::new()),
    });
    let mut options = DaemonOptions::new(env.paths.clone());
    options.settings = delayed.clone();
    let daemon = Daemon::new(options);
    daemon.start_workers();
    let mut monitor = daemon.subscribe();
    daemon.handle_tool_done(&tool_done());
    assert!(eventually(2, || delayed.calls.load(Ordering::SeqCst) >= 1).await);
    daemon.handle(session_end()).await;
    *delayed.gate.0.lock().unwrap() = true;
    delayed.gate.1.notify_all();
    daemon.settle().await;
    daemon.flush_now().await;
    let store = tkm_store::open_read_only(&env.paths.db).unwrap();
    let (ended, status): (Option<i64>, String) = store
        .query_row(
            "SELECT ended_at, fold_status FROM sessions WHERE session_id = ?1",
            [session_end().payload["session_id"].as_str().unwrap()],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert!(ended.is_some());
    assert_ne!(status, "open");
    while monitor.try_recv().is_ok() {}
    daemon.handle(stop()).await;
    assert!(
        monitor.try_recv().is_err(),
        "session was resurrected in memory"
    );
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
async fn disabled_facts_and_ledger_do_not_persist_fold_results() {
    let env = Env::new();
    let settings = Settings {
        facts_enabled: false,
        ledger_enabled: false,
        ..Settings::default()
    };
    let daemon = daemon_with(&env, settings, Arc::new(NullDecider));
    daemon.handle(start_request(&transcript())).await;
    daemon.handle(end_request(&transcript())).await;
    daemon.settle().await;
    let store = tkm_store::open_read_only(&env.paths.db).unwrap();
    for table in ["facts", "ledger"] {
        let rows: i64 = store
            .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))
            .unwrap();
        assert_eq!(rows, 0, "{table} must be disabled");
    }
    let status: String = store
        .query_row("SELECT fold_status FROM sessions", [], |r| r.get(0))
        .unwrap();
    assert_eq!(status, "folded");
}

#[tokio::test]
async fn fold_sends_one_candidate_batch_and_fails_open_on_decision_failure() {
    use tkm_decide::client::{NoDecisionReason, Outcome};
    let env = Env::new();
    let decider = ScriptedDecider::new(Outcome::NoDecision(NoDecisionReason::AllFailed));
    let daemon = daemon_with(&env, Settings::default(), decider.clone());
    let path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/replay-form-repair.jsonl");
    daemon.handle(start_request(path.to_str().unwrap())).await;
    daemon.handle(end_request(path.to_str().unwrap())).await;
    daemon.settle().await;
    let requests = decider.requests.lock().unwrap();
    let filters: Vec<_> = requests
        .iter()
        .filter(|request| request.purpose == tkm_decide::cache::Purpose::FactFilter)
        .collect();
    assert_eq!(filters.len(), 1);
    assert!(
        filters[0]
            .questions
            .keys()
            .any(|key| key.starts_with("stable_"))
    );
    assert!(
        filters[0]
            .questions
            .keys()
            .any(|key| key.starts_with("safe_"))
    );
    let store = tkm_store::open_read_only(&env.paths.db).unwrap();
    let count: i64 = store
        .query_row("SELECT COUNT(*) FROM facts", [], |row| row.get(0))
        .unwrap();
    assert!(count > 0, "failure must preserve code-only facts");
}

#[tokio::test]
async fn fold_applies_mocked_filter_verdicts_after_calibration() {
    use tkm_decide::api::Answer;
    use tkm_decide::client::Outcome;
    use tkm_hosts::{HostAdapter, SessionSource};
    let env = Env::new();
    let path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/replay-form-repair.jsonl");
    let session = tkm_hosts::claude::ClaudeAdapter::new()
        .read_session(&SessionSource {
            agent_home: "/fake/agent".into(),
            path: path.clone(),
            session_id: "test".into(),
            repo_key: tkm_core::model::RepoKey(FIXTURE_REPO.into()),
            cwd: "/fake/repo".into(),
        })
        .unwrap();
    let candidates =
        tkm_core::facts::extract::extract(&tkm_core::facts::observe::observe(&session));
    assert!(!candidates.is_empty());
    let mut decision = match stuck_decision() {
        Outcome::Decision(decision) => decision,
        _ => unreachable!(),
    };
    decision.answers.clear();
    for candidate in candidates {
        decision.answers.insert(
            format!("stable_{}", candidate.id),
            Answer::Noul { noul: 0.0 },
        );
        decision
            .answers
            .insert(format!("safe_{}", candidate.id), Answer::Noul { noul: 1.0 });
    }
    env.store().upsert_calibration(&tkm_store::CalibrationRecord {
        model_version: MODEL.into(), purpose: "fact_filter".into(),
        thresholds: serde_json::json!({"t_stuck":0.85,"t_progress":0.3,"t_low":0.3,"t_high":0.8}),
        sample_count: 200, precision: 0.95, recall: 0.6,
    }).unwrap();
    let settings = Settings {
        shadow_days: 0,
        shadow_turns: 0,
        ..Settings::default()
    };
    let daemon = daemon_with(
        &env,
        settings,
        ScriptedDecider::new(Outcome::Decision(decision)),
    );
    daemon.handle(start_request(path.to_str().unwrap())).await;
    daemon.handle(end_request(path.to_str().unwrap())).await;
    daemon.settle().await;
    let store = tkm_store::open_read_only(&env.paths.db).unwrap();
    let facts: i64 = store
        .query_row("SELECT COUNT(*) FROM facts", [], |r| r.get(0))
        .unwrap();
    assert_eq!(facts, 0, "unstable candidate must be rejected");
    let decisions: i64 = store
        .query_row(
            "SELECT COUNT(*) FROM decisions WHERE purpose='fact_filter' AND shadow=0",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(decisions, 1);
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
    daemon.flush_error_log().await;
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
    let store = tkm_store::open_read_only(&env.paths.db).unwrap();
    let status: String = store
        .query_row(
            "SELECT fold_status FROM sessions WHERE session_id = 'e928a2cf-e31b-4154-805b-919991d042ea'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(status, "folded");
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

#[tokio::test]
async fn fold_prices_session_with_configured_model_rates() {
    let env = Env::new();
    let mut settings = Settings::default();
    settings.pricing =
        tkm_core::ledger::PriceTable::parse(include_str!("../../../fixtures/pricing/pricing.toml"))
            .unwrap();
    let text = std::fs::read_to_string(transcript()).unwrap();
    let model = "example-model";
    let replaced = text
        .replace("claude-haiku-5-5", model)
        .replace("claude-opus-5-5", model);
    let path = env.dir.path().join("priced.jsonl");
    std::fs::write(&path, replaced).unwrap();
    let daemon = daemon_with(&env, settings, Arc::new(NullDecider));
    daemon.handle(start_request(path.to_str().unwrap())).await;
    daemon.handle(end_request(path.to_str().unwrap())).await;
    daemon.settle().await;
    let conn = tkm_store::open_read_only(&env.paths.db).unwrap();
    let cost: f64 = conn
        .query_row("SELECT priced_cost FROM ledger", [], |r| r.get(0))
        .unwrap();
    assert!(cost > 0.0);
}

#[tokio::test]
async fn disabled_host_does_not_create_or_fold_sessions() {
    let env = Env::new();
    let mut settings = Settings::default();
    settings.hosts.insert(
        "claude".into(),
        tkmd::settings::HostSettings {
            enabled: false,
            level: tkmd::settings::HostLevel::Auto,
        },
    );
    let daemon = daemon_with(&env, settings, Arc::new(NullDecider));
    daemon.handle(start_request(&transcript())).await;
    daemon.handle(end_request(&transcript())).await;
    daemon.settle().await;
    let store = env.store();
    assert!(
        store
            .list_sessions_by_repo(&tkm_core::model::RepoKey(FIXTURE_REPO.into()), 100)
            .unwrap()
            .is_empty()
    );
}
