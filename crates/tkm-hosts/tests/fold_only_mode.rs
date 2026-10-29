#![forbid(unsafe_code)]
//! Tests for `fold_only::mode_for` (`--no-shims`, ADR 0026).

use tkm_hosts::fold_only::{IntegrationMode, mode_for};
use tkm_proto::Host;

#[test]
fn shim_hosts_fold_only_without_shims() {
    for host in [Host::Opencode, Host::Pi] {
        assert_eq!(mode_for(host, false), IntegrationMode::FoldOnly);
        assert_eq!(mode_for(host, true), IntegrationMode::Shim);
    }
}

#[test]
fn hook_hosts_ignore_the_shim_flag() {
    for host in [
        Host::Claude,
        Host::Codex,
        Host::Agy,
        Host::Cursor,
        Host::Kimi,
        Host::Deepseek,
    ] {
        assert_eq!(mode_for(host, false), IntegrationMode::Hooks);
        assert_eq!(mode_for(host, true), IntegrationMode::Hooks);
    }
}
