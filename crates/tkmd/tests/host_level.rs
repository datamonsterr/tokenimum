#![forbid(unsafe_code)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::pedantic)]

use tkm_config::config::{LoadInputs, load};
use tkm_hosts::fold_only::IntegrationMode;
use tkm_proto::Host;
use tkm_store::Store;
use tkmd::host_level::effective_level;

#[test]
fn failed_probe_folds_and_success_restores_configured_level() {
    let store = Store::open_in_memory().unwrap();
    let config = load(&LoadInputs::default()).unwrap();
    assert_eq!(
        effective_level(&config, &store, Host::Claude).unwrap(),
        IntegrationMode::Hooks
    );
    store.record_host_health(Host::Claude, false).unwrap();
    assert_eq!(
        effective_level(&config, &store, Host::Claude).unwrap(),
        IntegrationMode::FoldOnly
    );
    // Other hosts are independent.
    assert_eq!(
        effective_level(&config, &store, Host::Cursor).unwrap(),
        IntegrationMode::Hooks
    );
    store.record_host_health(Host::Claude, true).unwrap();
    assert_eq!(
        effective_level(&config, &store, Host::Claude).unwrap(),
        IntegrationMode::Hooks
    );
}

#[test]
fn passing_probe_cannot_enable_a_disabled_or_fold_only_host() {
    let store = Store::open_in_memory().unwrap();
    for flag in ["hosts.claude.enabled=false", "hosts.claude.level=fold-only"] {
        let config = load(&LoadInputs {
            flags: vec![flag.into()],
            ..LoadInputs::default()
        })
        .unwrap();
        store.record_host_health(Host::Claude, true).unwrap();
        assert_eq!(
            effective_level(&config, &store, Host::Claude).unwrap(),
            IntegrationMode::FoldOnly
        );
    }
}

mod common;

#[tokio::test]
async fn daemon_suppresses_live_injection_when_doctor_forces_fold_only() {
    let env = common::Env::new();
    common::seed_fact(&env, "example.invalid");
    env.store().record_host_health(Host::Claude, false).unwrap();
    let daemon = common::daemon_with(
        &env,
        tkmd::settings::Settings::default(),
        std::sync::Arc::new(tkmd::decider::NullDecider),
    );
    daemon.start_workers();
    daemon.handle(common::session_start()).await;
    let reply = daemon.handle(common::prompt()).await;
    assert_eq!(reply.action, tkm_proto::ReplyAction::Nothing);
    daemon.flush_now().await;
    let sessions = env
        .store()
        .list_sessions_by_repo(&tkm_core::model::RepoKey(common::FIXTURE_REPO.into()), 10)
        .unwrap();
    assert_eq!(sessions[0].integration_level, "fold-only");
    daemon.shutdown().await;
}
