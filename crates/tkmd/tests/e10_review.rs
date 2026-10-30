//! E10 review: fold-only sessions, per-repository host config and the doctor health gate.
#![forbid(unsafe_code)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::pedantic)]

mod common;

use std::sync::Arc;

use common::*;
use tkm_core::model::RepoKey;
use tkm_proto::{Host, ReplyAction};
use tkmd::clock::ManualClock;
use tkmd::daemon::{Daemon, DaemonOptions};
use tkmd::decider::NullDecider;
use tkmd::settings::{FileSettings, HostLevel, HostSettings, Settings};

fn host_settings(enabled: bool, level: HostLevel) -> Settings {
    let mut settings = Settings::default();
    settings
        .hosts
        .insert("claude".into(), HostSettings { enabled, level });
    settings
}

fn sessions(env: &Env) -> Vec<tkm_store::SessionRecord> {
    env.store()
        .list_sessions_by_repo(&RepoKey(FIXTURE_REPO.into()), 10)
        .unwrap()
}

#[tokio::test]
async fn fold_only_level_keeps_the_session_for_folding_without_live_actions() {
    let env = Env::new();
    seed_fact(&env, "example.invalid");
    let daemon = daemon_with(
        &env,
        host_settings(true, HostLevel::FoldOnly),
        Arc::new(NullDecider),
    );
    daemon.start_workers();
    daemon.handle(session_start()).await;
    let reply = daemon.handle(prompt()).await;
    assert_eq!(reply.action, ReplyAction::Nothing);
    daemon.handle(stop()).await;
    daemon.flush_now().await;
    let rows = sessions(&env);
    assert_eq!(rows.len(), 1, "fold-only session must stay tracked");
    assert_eq!(rows[0].integration_level, "fold-only");
    daemon.shutdown().await;
}

#[tokio::test]
async fn disabled_host_still_drops_its_sessions() {
    let env = Env::new();
    let daemon = daemon_with(
        &env,
        host_settings(false, HostLevel::Auto),
        Arc::new(NullDecider),
    );
    daemon.start_workers();
    daemon.handle(session_start()).await;
    daemon.flush_now().await;
    assert!(sessions(&env).is_empty());
    daemon.shutdown().await;
}

#[tokio::test]
async fn repository_host_config_is_evaluated_per_session_across_repositories() {
    let env = Env::new();
    let repo_a = env.dir.path().join("repo-a");
    let repo_b = env.dir.path().join("repo-b");
    for repo in [&repo_a, &repo_b] {
        std::fs::create_dir_all(repo).unwrap();
    }
    std::fs::write(
        repo_a.join(".tokenimum.toml"),
        "[hosts.claude]\nlevel = \"fold-only\"\n",
    )
    .unwrap();
    let mut options = DaemonOptions::new(env.paths.clone());
    options.decider = Arc::new(NullDecider);
    options.settings = Arc::new(FileSettings::new(env.paths.clone(), Vec::new()));
    options.clock = Arc::new(ManualClock::new(1_800_000_000_000));
    let daemon = Daemon::new(options);
    daemon.start_workers();
    for (repo, id) in [(&repo_a, "session-a"), (&repo_b, "session-b")] {
        let mut start = session_start();
        start.payload["cwd"] = repo.to_str().unwrap().into();
        start.payload["session_id"] = id.into();
        daemon.handle(start).await;
    }
    daemon.flush_now().await;
    let level = |repo: &std::path::Path| {
        let key = tkm_config::repo::repo_identity(repo, "", None).key;
        let rows = env.store().list_sessions_by_repo(&key, 10).unwrap();
        assert_eq!(rows.len(), 1, "{}", repo.display());
        rows[0].integration_level.clone()
    };
    assert_eq!(level(&repo_a), "fold-only");
    assert_eq!(level(&repo_b), "hooks");
    assert!(!daemon.host_is_fold_only(Host::Claude));
    daemon.shutdown().await;
}

#[tokio::test]
async fn unsupported_host_level_degrades_to_fold_only() {
    let env = Env::new();
    seed_fact(&env, "example.invalid");
    // Claude has no shim: `shim` is not a supported combination.
    let daemon = daemon_with(
        &env,
        host_settings(true, HostLevel::Shim),
        Arc::new(NullDecider),
    );
    daemon.start_workers();
    daemon.handle(session_start()).await;
    assert_eq!(daemon.handle(prompt()).await.action, ReplyAction::Nothing);
    daemon.flush_now().await;
    assert_eq!(sessions(&env)[0].integration_level, "fold-only");
    daemon.shutdown().await;
}

#[tokio::test]
async fn folding_preserves_a_fold_only_sessions_integration_level() {
    let env = Env::new();
    let daemon = daemon_with(
        &env,
        host_settings(true, HostLevel::FoldOnly),
        Arc::new(NullDecider),
    );
    daemon.start_workers();
    daemon.handle(session_start()).await;
    daemon.handle(session_end()).await;
    daemon.settle().await;
    daemon.flush_now().await;
    let rows = sessions(&env);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].integration_level, "fold-only");
    daemon.shutdown().await;
}

#[tokio::test]
async fn doctor_fallback_keeps_configured_settings_and_recovery_restores_injection() {
    let env = Env::new();
    seed_fact(&env, "example.invalid");
    env.store().record_host_health(Host::Claude, false).unwrap();
    let daemon = daemon_with(&env, Settings::default(), Arc::new(NullDecider));
    daemon.start_workers();
    daemon.handle(session_start()).await;
    assert_eq!(daemon.handle(prompt()).await.action, ReplyAction::Nothing);
    env.store().record_host_health(Host::Claude, true).unwrap();
    daemon
        .refresh_host_health(&env.store(), Host::Claude)
        .unwrap();
    let reply = daemon.handle(prompt()).await;
    assert_ne!(
        reply.action,
        ReplyAction::Nothing,
        "recovery must re-enable injection"
    );
    daemon.shutdown().await;
}

#[tokio::test]
async fn doctor_downgrade_stops_injection_for_an_existing_session() {
    let env = Env::new();
    seed_fact(&env, "example.invalid");
    let daemon = daemon_with(&env, Settings::default(), Arc::new(NullDecider));
    daemon.start_workers();
    daemon.handle(session_start()).await;
    env.store().record_host_health(Host::Claude, false).unwrap();
    daemon
        .refresh_host_health(&env.store(), Host::Claude)
        .unwrap();
    let reply = daemon.handle(prompt()).await;
    assert_eq!(reply.action, ReplyAction::Nothing);
    daemon.shutdown().await;
}
