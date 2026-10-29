//! SPEC 16 fault-injection matrix, daemon side: one test per fault. Each must give the no-op
//! reply and one `errors.log` line, and the daemon must keep serving. Daemon down, slow socket
//! and shim timeout are hook-side faults, in `crates/tkm-hook/tests/fault_matrix.rs`.
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

/// Drives 22 failing tool calls against a fake OpenRouter and returns the error-log lines.
async fn openrouter_fault(
    status: u16,
    body: &str,
    delay: Duration,
    timeout_ms: u64,
) -> Vec<String> {
    let env = Env::new();
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
    let decider = build_decider(
        config,
        Some(key),
        Some(&url),
        Arc::new(StoreLedger::default()),
    );
    let settings = Settings {
        loop_mode: LoopMode::Redirect,
        ..Settings::default()
    };
    env.seed_envelope(FIXTURE_REPO, Host::Claude, &[5; 5]);
    let daemon = daemon_with(&env, settings, decider);
    daemon.start_workers();
    daemon.handle(session_start()).await;
    daemon.handle(prompt()).await;
    let replies: Vec<_> = (0..22)
        .map(|_| daemon.handle_tool_done(&failing_tool_done()))
        .collect();
    daemon.settle().await;
    assert!(
        replies
            .iter()
            .all(|r| r.stdout.is_empty() && r.action == ReplyAction::Nothing),
        "every hook reply is the no-op"
    );
    assert_eq!(daemon.handle(stop()).await.stdout, "");
    daemon.flush_error_log().await;
    assert!(
        hits.load(std::sync::atomic::Ordering::SeqCst) >= 1,
        "model was called"
    );
    env.error_lines()
}

#[tokio::test]
async fn fault_openrouter_404() {
    let lines = openrouter_fault(
        404,
        "{\"error\":{\"code\":404,\"message\":\"no model\"}}",
        Duration::ZERO,
        1_500,
    )
    .await;
    assert_eq!(lines.len(), 1, "{lines:?}");
}

#[tokio::test]
async fn fault_openrouter_500() {
    let lines = openrouter_fault(
        500,
        "{\"error\":{\"code\":500,\"message\":\"boom\"}}",
        Duration::ZERO,
        1_500,
    )
    .await;
    assert_eq!(lines.len(), 1, "{lines:?}");
}

#[tokio::test]
async fn fault_openrouter_timeout() {
    let lines = openrouter_fault(200, &decision_body(), Duration::from_secs(3), 200).await;
    assert_eq!(lines.len(), 1, "{lines:?}");
}

#[tokio::test]
async fn fault_db_locked() {
    let env = Env::new();
    drop(env.store());
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
    assert_eq!(daemon.handle(stop()).await.stdout, "");
    lock.execute_batch("ROLLBACK").unwrap();
}

#[tokio::test]
async fn fault_corrupt_log() {
    let env = Env::new();
    let daemon = daemon_with(&env, Settings::default(), Arc::new(NullDecider));
    // An unreadable transcript (a directory) is an I/O error: one line, session marked "error".
    let dir = env.dir.path().join("transcript-dir");
    std::fs::create_dir_all(&dir).unwrap();
    let mut payload = fixture("SessionStart");
    payload["transcript_path"] = serde_json::json!(dir.to_string_lossy());
    assert_eq!(
        daemon
            .handle(request(EventKind::SessionStart, payload))
            .await
            .stdout,
        ""
    );
    let mut payload = fixture("SessionEnd");
    payload["transcript_path"] = serde_json::json!(dir.to_string_lossy());
    let reply = daemon.handle(request(EventKind::SessionEnd, payload)).await;
    assert_eq!(reply.stdout, "");
    assert_eq!(reply.action, ReplyAction::Nothing);
    daemon.settle().await;
    daemon.flush_error_log().await;
    assert_eq!(env.error_lines().len(), 1, "{:?}", env.error_lines());
    // The daemon keeps serving.
    assert_eq!(daemon.handle(prompt()).await.stdout, "");
}

/// Reads a foreign SQLite database read-only, like the agy and opencode readers.
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
        let io = |e: rusqlite::Error| AdapterError::Io(std::io::Error::other(e.to_string()));
        let conn = rusqlite::Connection::open_with_flags(
            &self.db,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .map_err(io)?;
        conn.busy_timeout(Duration::from_millis(50)).map_err(io)?;
        let _: i64 = conn
            .query_row("SELECT COUNT(*) FROM chats", [], |r| r.get(0))
            .map_err(io)?;
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
async fn fault_foreign_sqlite_locked() {
    let env = Env::new();
    let foreign = env.dir.path().join("foreign.db");
    let setup = rusqlite::Connection::open(&foreign).unwrap();
    setup
        .execute_batch(
            "PRAGMA journal_mode=DELETE; CREATE TABLE chats(id INTEGER); INSERT INTO chats VALUES (1);",
        )
        .unwrap();
    setup.execute_batch("BEGIN EXCLUSIVE").unwrap();
    let daemon = daemon_with(&env, Settings::default(), Arc::new(NullDecider));
    daemon
        .adapters()
        .register(Box::new(ForeignSqlite { db: foreign }));
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
    assert_eq!(daemon.handle(session_start()).await.stdout, "");
    setup.execute_batch("ROLLBACK").unwrap();
}
