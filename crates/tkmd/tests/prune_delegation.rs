//! Delegated session observation reaches pruning history through the daemon outbox.
#![forbid(unsafe_code)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::pedantic)]
mod common;
use common::*;
use tkm_core::model::{RepoKey, Timestamp};
use tkmd::settings::Settings;

#[tokio::test]
async fn delegated_session_is_recorded_once_with_daemon_clock() {
    let env = Env::new();
    let daemon = daemon_with(
        &env,
        Settings::default(),
        ScriptedDecider::new(stuck_decision()),
    );
    let mut start = session_start();
    start.env.insert("TKM_DELEGATED".into(), "1".into());
    daemon.handle(start.clone()).await;
    daemon.flush_now().await;
    daemon.handle(start).await;
    daemon.flush_now().await;
    let store = env.store();
    let repo = RepoKey(FIXTURE_REPO.into());
    assert_eq!(store.get_repo(&repo).unwrap().unwrap().delegated_runs, 1);
    assert_eq!(
        store
            .last_delegated_run_within(&repo, Timestamp(1_800_000_000_000))
            .unwrap(),
        Some(Timestamp(1_800_000_000_000))
    );
}
