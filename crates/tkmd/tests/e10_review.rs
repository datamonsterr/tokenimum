//! E10 review: fold-only sessions, per-repository host config and the doctor health gate.
#![forbid(unsafe_code)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::pedantic)]

mod common;

use std::sync::Arc;

use common::*;
use tkm_core::model::RepoKey;
use tkm_proto::{Host, ReplyAction};
use tkmd::decider::NullDecider;
use tkmd::settings::{HostLevel, HostSettings, Settings};

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
async fn repository_host_config_does_not_leak_into_the_shared_doctor_cache() {
    for config in [
        "[hosts.claude]\nlevel = \"fold-only\"\n",
        "[hosts.claude]\nenabled = false\n",
    ] {
        let env = Env::new();
        let repo_a = env.dir.path().join("repo-a");
        std::fs::create_dir_all(&repo_a).unwrap();
        std::fs::write(repo_a.join(".tokenimum.toml"), config).unwrap();
        let daemon = daemon_with(&env, Settings::default(), Arc::new(NullDecider));
        daemon
            .refresh_host_health(&env.store(), Host::Claude)
            .unwrap();
        assert!(
            !daemon.host_is_fold_only(Host::Claude),
            "repo A config must not gate repo B sessions: {config}"
        );
    }
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
