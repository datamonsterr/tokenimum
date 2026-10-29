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
        Ok(())
    }

    pub(crate) fn host_is_fold_only(&self, host: Host) -> bool {
        crate::daemon::lock(&self.host_fallback)
            .get(&host)
            .copied()
            .unwrap_or(false)
    }
}
