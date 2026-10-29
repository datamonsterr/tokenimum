//! E4.3 hot-path rule (SPEC 6.2, 14): `tool_done` does no network call and no disk access.
#![forbid(unsafe_code)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::pedantic)]

mod common;

use std::sync::Arc;

use common::*;
use tkm_proto::Host;
use tkmd::settings::Settings;

#[cfg(target_os = "linux")]
fn thread_write_calls() -> u64 {
    std::fs::read_to_string("/proc/thread-self/io")
        .unwrap()
        .lines()
        .find_map(|line| {
            line.strip_prefix("syscw: ")
                .map(|value| value.trim().parse().unwrap())
        })
        .unwrap()
}

#[cfg(target_os = "linux")]
#[tokio::test]
async fn malformed_tool_done_does_not_open_storage_call_decider_or_write_on_handler_thread() {
    let env = Env::new();
    let backing = CountingBacking::new(env.paths.db.clone());
    let decider = ScriptedDecider::new(stuck_decision());
    let mut options = tkmd::daemon::DaemonOptions::new(env.paths.clone());
    options.backing = backing.clone();
    options.decider = decider.clone();
    let daemon = tkmd::daemon::Daemon::new(options);
    let mut malformed = tool_done();
    malformed.payload["tool_input"] = serde_json::Value::Null;
    let opens = backing.opens();
    let writes = thread_write_calls();
    assert_eq!(daemon.handle_tool_done(&malformed).stdout, "");
    assert_eq!(thread_write_calls(), writes);
    assert_eq!(backing.opens(), opens);
    assert_eq!(decider.calls(), 0);
    daemon.flush_error_log().await;
    assert_eq!(env.error_lines().len(), 1);
}

#[cfg(target_os = "linux")]
#[tokio::test]
async fn judge_enqueue_does_not_open_storage_call_decider_or_write_on_handler_thread() {
    use tkm_proto::monitor::MonitorMessage;
    let env = Env::new();
    env.seed_envelope(FIXTURE_REPO, Host::Claude, &[5; 5]);
    let backing = CountingBacking::new(env.paths.db.clone());
    let decider = ScriptedDecider::new(stuck_decision());
    let mut options = tkmd::daemon::DaemonOptions::new(env.paths.clone());
    options.backing = backing.clone();
    options.decider = decider.clone();
    options.settings = settings_fixed(Settings::default());
    let daemon = tkmd::daemon::Daemon::new(options);
    daemon.handle(session_start()).await;
    daemon.handle(prompt()).await;
    for _ in 0..17 {
        daemon.handle_tool_done(&failing_tool_done());
    }
    let mut monitor = daemon.subscribe();
    let opens = backing.opens();
    let writes = thread_write_calls();
    daemon.handle_tool_done(&failing_tool_done());
    assert_eq!(thread_write_calls(), writes);
    assert_eq!(backing.opens(), opens);
    assert_eq!(decider.calls(), 0);
    assert!(
        matches!(monitor.try_recv(), Ok(MonitorMessage::Session(update)) if update.loop_state == "judging")
    );
}

#[tokio::test]
async fn tool_done_makes_no_network_call_and_touches_no_storage() {
    let env = Env::new();
    env.seed_envelope(FIXTURE_REPO, Host::Claude, &[5; 5]);
    let backing = CountingBacking::new(env.paths.db.clone());
    let decider = ScriptedDecider::new(stuck_decision());
    let mut options = tkmd::daemon::DaemonOptions::new(env.paths.clone());
    options.backing = backing.clone();
    options.decider = decider.clone();
    options.settings = settings_fixed(Settings::default());
    let daemon = tkmd::daemon::Daemon::new(options);

    // Setup events may read the database.
    daemon.handle(session_start()).await;
    daemon.handle(prompt()).await;

    let opens_before = backing.opens();
    let files_before = dir_listing(&env.paths.data_dir);
    let mtime_before = std::fs::metadata(&env.paths.db)
        .unwrap()
        .modified()
        .unwrap();
    #[cfg(target_os = "linux")]
    let writes_before = thread_write_calls();
    for _ in 0..17 {
        let reply = daemon.handle_tool_done(&tool_done());
        assert_eq!(reply.stdout, "");
    }
    #[cfg(target_os = "linux")]
    assert_eq!(
        thread_write_calls(),
        writes_before,
        "no handler-thread writes"
    );
    assert_eq!(
        backing.opens(),
        opens_before,
        "no storage access on tool_done"
    );
    assert_eq!(decider.calls(), 0, "no network on tool_done");
    assert_eq!(
        dir_listing(&env.paths.data_dir),
        files_before,
        "no new files"
    );
    assert_eq!(
        std::fs::metadata(&env.paths.db)
            .unwrap()
            .modified()
            .unwrap(),
        mtime_before,
        "no database write"
    );
    assert!(env.error_lines().is_empty());
}

#[tokio::test]
async fn the_flusher_is_what_writes_state_afterwards() {
    let env = Env::new();
    let backing = CountingBacking::new(env.paths.db.clone());
    let mut options = tkmd::daemon::DaemonOptions::new(env.paths.clone());
    options.backing = backing.clone();
    options.settings = settings_fixed(Settings::default());
    let daemon = tkmd::daemon::Daemon::new(options);
    daemon.handle(session_start()).await;
    daemon.handle(prompt()).await;
    for _ in 0..3 {
        daemon.handle_tool_done(&tool_done());
    }
    let opens = backing.opens();
    daemon.flush_now().await;
    assert!(
        backing.opens() > opens,
        "flush opens the store off the request path"
    );
    let store = tkm_store::open_read_only(&env.paths.db).unwrap();
    let sessions: i64 = store
        .query_row("SELECT COUNT(*) FROM sessions", [], |r| r.get(0))
        .unwrap();
    assert_eq!(sessions, 1);
    let _ = Arc::strong_count(&backing);
}

fn dir_listing(dir: &std::path::Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}
