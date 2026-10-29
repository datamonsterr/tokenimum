//! Effective host integration levels, resolved outside the tool hot path.

use tkm_config::config::EffectiveConfig;
use tkm_hosts::fold_only::{IntegrationMode, effective_mode};
use tkm_proto::Host;
use tkm_store::{Store, StoreError};

/// Resolve configuration through the persisted doctor fallback.
pub fn effective_level(
    config: &EffectiveConfig,
    store: &Store,
    host: Host,
) -> Result<IntegrationMode, StoreError> {
    Ok(effective_mode(
        host,
        config
            .get_str(&format!("hosts.{}.level", host.as_str()))
            .unwrap_or("auto"),
        config
            .get_bool(&format!("hosts.{}.enabled", host.as_str()))
            .unwrap_or(true),
        store.host_fold_only(host)?,
    ))
}

impl crate::daemon::Daemon {
    pub(crate) fn refresh_host_health(
        &self,
        store: &Store,
        host: Host,
        root: &std::path::Path,
    ) -> Result<(), StoreError> {
        let config = tkm_config::config::load(&tkm_config::config::LoadInputs {
            repo_root: Some(root.to_path_buf()),
            global_path: self.paths.global_config.clone(),
            env: self.env.clone(),
            ..Default::default()
        });
        let folded = match config {
            Ok(config) => effective_level(&config, store, host)? == IntegrationMode::FoldOnly,
            Err(_) => store.host_fold_only(host)?,
        };
        crate::daemon::lock(&self.host_fallback).insert(host, folded);
        let events = store
            .host_event_probes(host)?
            .into_iter()
            .filter(|event| !event.arrived)
            .map(|event| event.native)
            .collect();
        crate::daemon::lock(&self.idle_events).insert(host, events);
        Ok(())
    }

    pub(crate) fn host_is_fold_only(&self, host: Host) -> bool {
        crate::daemon::lock(&self.host_fallback)
            .get(&host)
            .copied()
            .unwrap_or(false)
    }
}

impl crate::daemon::Daemon {
    /// Whether doctor routes this native event to the idle fold worker.
    pub fn event_uses_idle(
        &self,
        host: Host,
        native: Option<&str>,
        kind: tkm_proto::EventKind,
    ) -> bool {
        let policies = crate::daemon::lock(&self.idle_events);
        let Some(missing) = policies.get(&host) else {
            return false;
        };
        native.map_or_else(
            || {
                tkm_hosts::probe::expected_events(host)
                    .iter()
                    .any(|event| event.kind == kind && missing.contains(event.native))
            },
            |native| missing.contains(native),
        )
    }
}
