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
    assert_eq!(effective_level(&config, &store, Host::Claude).unwrap(), IntegrationMode::Hooks);
    store.record_host_health(Host::Claude, false).unwrap();
    assert_eq!(effective_level(&config, &store, Host::Claude).unwrap(), IntegrationMode::FoldOnly);
    // Other hosts are independent.
    assert_eq!(effective_level(&config, &store, Host::Cursor).unwrap(), IntegrationMode::Hooks);
    store.record_host_health(Host::Claude, true).unwrap();
    assert_eq!(effective_level(&config, &store, Host::Claude).unwrap(), IntegrationMode::Hooks);
}

#[test]
fn passing_probe_cannot_enable_a_disabled_or_fold_only_host() {
    let store = Store::open_in_memory().unwrap();
    for flag in ["hosts.claude.enabled=false", "hosts.claude.level=fold-only"] {
        let config = load(&LoadInputs { flags: vec![flag.into()], ..LoadInputs::default() }).unwrap();
        store.record_host_health(Host::Claude, true).unwrap();
        assert_eq!(effective_level(&config, &store, Host::Claude).unwrap(), IntegrationMode::FoldOnly);
    }
}
