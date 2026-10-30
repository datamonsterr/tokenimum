#![forbid(unsafe_code)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::pedantic)]
mod common;
use tkm_proto::{EventKind, Host};

#[test]
fn event_policy_preserves_native_event_independence_and_recovers() {
    let store = tkm_store::Store::open_in_memory().unwrap();
    store
        .record_host_event_probe(Host::Claude, "PostToolUse", EventKind::ToolDone, true, 1)
        .unwrap();
    store
        .record_host_event_probe(
            Host::Claude,
            "PostToolUseFailure",
            EventKind::ToolDone,
            false,
            1,
        )
        .unwrap();
    assert!(
        store
            .host_event_probes(Host::Claude)
            .unwrap()
            .iter()
            .any(|r| !r.arrived)
    );
    store
        .record_host_event_probe(
            Host::Claude,
            "PostToolUseFailure",
            EventKind::ToolDone,
            true,
            2,
        )
        .unwrap();
    assert!(
        store
            .host_event_probes(Host::Claude)
            .unwrap()
            .iter()
            .all(|r| r.arrived)
    );
}

#[tokio::test]
async fn missing_session_end_is_folded_at_idle_instead_of_hook_dispatch() {
    use std::sync::Arc;
    use std::time::Duration;
    use tkmd::clock::ManualClock;
    let env = common::Env::new();
    let store = env.store();
    store
        .record_host_event_probe(Host::Claude, "SessionEnd", EventKind::SessionEnd, false, 1)
        .unwrap();
    let clock = Arc::new(ManualClock::new(1_800_000_000_000));
    let mut options = tkmd::daemon::DaemonOptions::new(env.paths.clone());
    options.clock = clock.clone();
    options.idle_after = Duration::from_secs(1);
    let daemon = tkmd::daemon::Daemon::new(options);
    daemon.start_workers();
    daemon.handle(common::session_start()).await;
    assert!(daemon.event_uses_idle(Host::Claude, Some("SessionEnd"), EventKind::SessionEnd));
    daemon.handle(common::session_end()).await;
    daemon.flush_now().await;
    let repo = tkm_core::model::RepoKey(common::FIXTURE_REPO.into());
    assert_eq!(
        store.list_sessions_by_repo(&repo, 10).unwrap()[0].fold_status,
        "open"
    );
    clock.advance(Duration::from_secs(2));
    daemon.scan_idle().await;
    daemon.settle().await;
    daemon.flush_now().await;
    assert_ne!(
        store.list_sessions_by_repo(&repo, 10).unwrap()[0].fold_status,
        "open"
    );
    daemon.shutdown().await;
}
