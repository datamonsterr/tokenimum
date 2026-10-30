#![forbid(unsafe_code)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::pedantic)]
use tkm_core::{
    model::{Host, RepoKey, Timestamp},
    prune::PruneConfig,
};
use tkm_store::{Repo, Store};

#[test]
fn readiness_starts_once_at_threshold_and_resets_on_use_or_threshold_change() {
    let store = Store::open_in_memory().unwrap();
    let repo = RepoKey("prune-clock".into());
    store
        .upsert_repo(&Repo {
            repo_key: repo.clone(),
            display_name: "test".into(),
            first_seen: Timestamp(0),
            last_seen: Timestamp(0),
            delegated_runs: 0,
        })
        .unwrap();
    let config = PruneConfig::default();
    for i in 1..=5 {
        store
            .record_tool_unused_session(&repo, Host::Cursor, "mcp__example__unused")
            .unwrap();
        store
            .refresh_prune_readiness(&repo, Host::Cursor, &config, Timestamp(i * 100))
            .unwrap();
        assert_eq!(
            store
                .prune_ready_since(&repo, Host::Cursor, "mcp__example__unused")
                .unwrap(),
            (i == 5).then_some(Timestamp(500))
        );
    }
    store
        .refresh_prune_readiness(&repo, Host::Cursor, &config, Timestamp(900))
        .unwrap();
    assert_eq!(
        store
            .prune_ready_since(&repo, Host::Cursor, "mcp__example__unused")
            .unwrap(),
        Some(Timestamp(500))
    );
    store
        .record_tool_used(&repo, Host::Cursor, "mcp__example__unused", Timestamp(1000))
        .unwrap();
    assert_eq!(
        store
            .prune_ready_since(&repo, Host::Cursor, "mcp__example__unused")
            .unwrap(),
        None
    );
    for _ in 0..5 {
        store
            .record_tool_unused_session(&repo, Host::Cursor, "mcp__example__unused")
            .unwrap();
    }
    store
        .refresh_prune_readiness(&repo, Host::Cursor, &config, Timestamp(2000))
        .unwrap();
    assert_eq!(
        store
            .prune_ready_since(&repo, Host::Cursor, "mcp__example__unused")
            .unwrap(),
        Some(Timestamp(2000))
    );
    let stricter = PruneConfig {
        sessions_any: 6,
        ..config
    };
    store
        .refresh_prune_readiness(&repo, Host::Cursor, &stricter, Timestamp(3000))
        .unwrap();
    assert_eq!(
        store
            .prune_ready_since(&repo, Host::Cursor, "mcp__example__unused")
            .unwrap(),
        None
    );
}
