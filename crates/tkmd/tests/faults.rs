//! Fault injection: each failure yields the host's no-op reply and one `errors.log` line, and the
//! daemon keeps serving (SPEC 13.4, EPICS E4 DoD).
#![forbid(unsafe_code)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::pedantic)]

mod common;

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use common::*;
use tkm_core::model::{HookEvent, Session};
use tkm_decide::config::{ChainModel, DecideConfig};
use tkm_decide::key::load_key;
use tkm_hosts::{
    AdapterError, HostAdapter, InstallContext, InstallReport, PruneTarget, SessionSource,
};
use tkm_proto::{EventKind, Host, ReplyAction, Request};
use tkmd::backing::SqliteBacking;
use tkmd::daemon::{Daemon, DaemonOptions};
use tkmd::decider::{NullDecider, StoreLedger, build_decider};
use tkmd::settings::{LoopMode, Settings};

#[tokio::test]
async fn shutdown_flushes_one_log_line_for_each_malformed_tool_event() {
    let env = Env::new();
    let daemon = daemon_with(&env, Settings::default(), Arc::new(NullDecider));
    let mut malformed = tool_done();
    malformed.payload["tool_input"] = serde_json::Value::Null;
    for _ in 0..8 {
        assert_eq!(daemon.handle_tool_done(&malformed).stdout, "");
    }
    daemon.shutdown().await;
    assert_eq!(env.error_lines().len(), 8);
}

#[tokio::test]
async fn database_locked_keeps_replying_and_logs_one_line_then_recovers() {
    let env = Env::new();
    drop(env.store()); // create the schema
    let mut options = DaemonOptions::new(env.paths.clone());
    options.backing = Arc::new(SqliteBacking::with_busy_timeout(
        env.paths.db.clone(),
        Duration::from_millis(50),
    ));
    options.settings = settings_fixed(Settings::default());
    options.decider = Arc::new(NullDecider);
    let daemon = Daemon::new(options);

    let lock = rusqlite::Connection::open(&env.paths.db).unwrap();
    lock.execute_batch("BEGIN IMMEDIATE").unwrap();

    assert_eq!(daemon.handle(session_start()).await.stdout, "");
    assert_eq!(daemon.handle(prompt()).await.stdout, "");
    assert_eq!(daemon.handle_tool_done(&tool_done()).stdout, "");
    daemon.flush_now().await;
    daemon.flush_error_log().await;
    assert_eq!(env.error_lines().len(), 1, "{:?}", env.error_lines());

    // Daemon still serves while the lock is held.
    assert_eq!(daemon.handle(stop()).await.stdout, "");
    lock.execute_batch("ROLLBACK").unwrap();
    drop(lock);

    // The unflushed state is retried, not lost.
    daemon.flush_now().await;
    let store = tkm_store::open_read_only(&env.paths.db).unwrap();
    let sessions: i64 = store
        .query_row("SELECT COUNT(*) FROM sessions", [], |r| r.get(0))
        .unwrap();
    assert_eq!(sessions, 1);
}

#[tokio::test]
async fn unreadable_database_is_a_noop_reply_with_a_logged_line() {
    let env = Env::new();
    std::fs::create_dir_all(&env.paths.data_dir).unwrap();
    std::fs::write(
        &env.paths.db,
        b"this is not a sqlite database at all, just text",
    )
    .unwrap();
    let daemon = daemon_with(&env, Settings::default(), Arc::new(NullDecider));
    assert_eq!(daemon.handle(session_start()).await.stdout, "");
    assert_eq!(daemon.handle(prompt()).await.stdout, "");
    daemon.flush_error_log().await;
    assert!(!env.error_lines().is_empty());
    assert_eq!(
        daemon.handle(session_start()).await.action,
        ReplyAction::Nothing
    );
}

async fn judge_against(
    env: &Env,
    status: u16,
    body: &str,
    delay: Duration,
    timeout_ms: u64,
) -> usize {
    let (url, hits) = fake_openrouter(status, body.to_owned(), delay).await;
    let config = DecideConfig {
        chain: vec![ChainModel {
            slug: "typesafe/jev-1.14-20261014".into(),
            free_probe: false,
        }],
        timeout_ms,
        ..DecideConfig::default()
    };
    let key = load_key(
        Some("sk-or-test".into()),
        std::path::Path::new("/nonexistent"),
        None,
    )
    .unwrap();
    let ledger = Arc::new(StoreLedger::default());
    let decider = build_decider(config, Some(key), Some(&url), ledger);
    let settings = Settings {
        loop_mode: LoopMode::Redirect,
        ..Settings::default()
    };
    env.seed_envelope(FIXTURE_REPO, Host::Claude, &[5; 5]);
    let daemon = daemon_with(env, settings, decider);
    daemon.start_workers();
    daemon.handle(session_start()).await;
    daemon.handle(prompt()).await;
    let mut replies = Vec::new();
    for _ in 0..22 {
        replies.push(daemon.handle_tool_done(&failing_tool_done()));
    }
    daemon.settle().await;
    assert!(
        replies
            .iter()
            .all(|r| r.stdout.is_empty() && r.action == ReplyAction::Nothing)
    );
    assert_eq!(daemon.handle(stop()).await.stdout, "");
    daemon.flush_error_log().await;
    hits.load(std::sync::atomic::Ordering::SeqCst)
}

#[tokio::test]
async fn openrouter_500_is_a_noop_with_one_errors_log_line() {
    let env = Env::new();
    let hits = judge_against(
        &env,
        500,
        "{\"error\":{\"code\":500,\"message\":\"boom\"}}",
        Duration::ZERO,
        1_500,
    )
    .await;
    assert!(hits >= 1, "errors.log: {:?}", env.error_lines());
    assert_eq!(env.error_lines().len(), 1, "{:?}", env.error_lines());
}

#[tokio::test]
async fn openrouter_404_is_a_noop_with_one_errors_log_line() {
    let env = Env::new();
    let hits = judge_against(
        &env,
        404,
        "{\"error\":{\"code\":404,\"message\":\"no model\"}}",
        Duration::ZERO,
        1_500,
    )
    .await;
    assert!(hits >= 1, "errors.log: {:?}", env.error_lines());
    assert_eq!(env.error_lines().len(), 1, "{:?}", env.error_lines());
}

#[tokio::test]
async fn openrouter_timeout_is_a_noop_with_one_errors_log_line() {
    let env = Env::new();
    let hits = judge_against(&env, 200, &decision_body(), Duration::from_secs(3), 200).await;
    assert!(hits >= 1, "errors.log: {:?}", env.error_lines());
    assert_eq!(env.error_lines().len(), 1, "{:?}", env.error_lines());
}

#[tokio::test]
async fn openrouter_success_through_the_real_client_is_recorded() {
    let env = Env::new();
    let (url, _hits) = fake_openrouter(200, decision_body(), Duration::ZERO).await;
    let config = DecideConfig {
        chain: vec![ChainModel {
            slug: "typesafe/jev-1.14-20261014".into(),
            free_probe: false,
        }],
        ..DecideConfig::default()
    };
    let key = load_key(
        Some("sk-or-test".into()),
        std::path::Path::new("/nonexistent"),
        None,
    )
    .unwrap();
    let decider = build_decider(
        config,
        Some(key),
        Some(&url),
        Arc::new(StoreLedger::default()),
    );
    env.seed_envelope(FIXTURE_REPO, Host::Claude, &[5; 5]);
    let settings = Settings {
        loop_mode: LoopMode::Redirect,
        ..Settings::default()
    };
    let daemon = daemon_with(&env, settings, decider);
    daemon.start_workers();
    daemon.handle(session_start()).await;
    daemon.handle(prompt()).await;
    for _ in 0..18 {
        daemon.handle_tool_done(&failing_tool_done());
    }
    daemon.settle().await;
    daemon.flush_now().await;
    let store = tkm_store::open_read_only(&env.paths.db).unwrap();
    let n: i64 = store
        .query_row("SELECT COUNT(*) FROM decisions WHERE shadow = 1", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(n, 1, "uncalibrated model: decision stored as shadow");
    assert!(env.error_lines().is_empty());
}

/// Reads a foreign SQLite database read-only, like the agy and opencode readers will.
struct ForeignSqlite {
    db: PathBuf,
}

impl HostAdapter for ForeignSqlite {
    fn host(&self) -> Host {
        Host::Agy
    }
    fn map_event(&self, request: &Request) -> Result<Option<HookEvent>, AdapterError> {
        let mut event = HookEvent::new(Host::Agy, request.event, "agy-1");
        event.cwd = Some(PathBuf::from("/work/agy"));
        Ok(Some(event))
    }
    fn render_reply(&self, _request: &Request, _action: &ReplyAction) -> String {
        "{}".into()
    }
    fn read_session(&self, _source: &SessionSource) -> Result<Session, AdapterError> {
        let conn = rusqlite::Connection::open_with_flags(
            &self.db,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .map_err(|e| AdapterError::Io(std::io::Error::other(e.to_string())))?;
        conn.busy_timeout(Duration::from_millis(50)).unwrap();
        let _: i64 = conn
            .query_row("SELECT COUNT(*) FROM chats", [], |r| r.get(0))
            .map_err(|e| AdapterError::Io(std::io::Error::other(e.to_string())))?;
        Ok(Session::new(Host::Agy))
    }
    fn install(&self, c: &InstallContext) -> Result<InstallReport, AdapterError> {
        Ok(InstallReport {
            dry_run: c.dry_run,
            ..InstallReport::default()
        })
    }
    fn uninstall(&self, c: &InstallContext) -> Result<InstallReport, AdapterError> {
        self.install(c)
    }
    fn prune_target(&self) -> PruneTarget {
        PruneTarget::ReportOnly
    }
}

#[tokio::test]
async fn foreign_sqlite_locked_during_fold_logs_one_line_and_the_daemon_survives() {
    let env = Env::new();
    let foreign = env.dir.path().join("foreign.db");
    let setup = rusqlite::Connection::open(&foreign).unwrap();
    setup.execute_batch("PRAGMA journal_mode=DELETE; CREATE TABLE chats(id INTEGER); INSERT INTO chats VALUES (1);").unwrap();
    setup.execute_batch("BEGIN EXCLUSIVE").unwrap(); // held for the whole test

    let daemon = daemon_with(&env, Settings::default(), Arc::new(NullDecider));
    daemon.adapters().register(Box::new(ForeignSqlite {
        db: foreign.clone(),
    }));
    let mut start = session_start();
    start.host = Host::Agy;
    let mut end = session_end();
    end.host = Host::Agy;
    end.event = EventKind::SessionEnd;
    assert_eq!(daemon.handle(start).await.stdout, "{}");
    assert_eq!(daemon.handle(end).await.stdout, "{}");
    daemon.settle().await;
    daemon.flush_error_log().await;
    assert_eq!(env.error_lines().len(), 1, "{:?}", env.error_lines());
    // Still serving other hosts.
    assert_eq!(daemon.handle(session_start()).await.stdout, "");
    setup.execute_batch("ROLLBACK").unwrap();
}

#[tokio::test]
async fn malformed_host_payload_is_a_noop_with_one_line() {
    let env = Env::new();
    let daemon = daemon_with(&env, Settings::default(), Arc::new(NullDecider));
    let reply = daemon
        .handle(request(
            EventKind::ToolDone,
            serde_json::json!({"nope": true}),
        ))
        .await;
    assert_eq!(reply.stdout, "");
    assert_eq!(reply.action, ReplyAction::Nothing);
    daemon.flush_error_log().await;
    assert_eq!(env.error_lines().len(), 1);
}
