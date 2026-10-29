//! Host adapter registry (SPEC 5.1 router).

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use tkm_hosts::agy::AgyAdapter;
use tkm_hosts::claude::ClaudeAdapter;
use tkm_hosts::codewhale::CodewhaleAdapter;
use tkm_hosts::codex::CodexAdapter;
use tkm_hosts::cursor::CursorAdapter;
use tkm_hosts::kimi::KimiAdapter;
use tkm_hosts::opencode::OpencodeAdapter;
use tkm_hosts::pi::PiAdapter;
use tkm_hosts::{HostAdapter, NoopAdapter};
use tkm_proto::Host;

/// Maps a host to its adapter. Hosts without an adapter get [`NoopAdapter`], which renders the
/// host's exact no-op reply.
pub struct AdapterRegistry {
    adapters: RwLock<HashMap<Host, Arc<dyn HostAdapter>>>,
}

impl AdapterRegistry {
    /// Registers all available host adapters; unregistered hosts use the no-op fallback.
    pub fn standard() -> Self {
        let registry = Self {
            adapters: RwLock::new(HashMap::new()),
        };
        registry.register(Box::new(ClaudeAdapter::new()));
        registry.register(Box::new(CodexAdapter));
        registry.register(Box::new(AgyAdapter::new()));
        registry.register(Box::new(CursorAdapter::new()));
        registry.register(Box::new(KimiAdapter::new()));
        registry.register(Box::new(CodewhaleAdapter::new()));
        registry.register(Box::new(OpencodeAdapter::new()));
        registry.register(Box::new(PiAdapter::new()));
        registry
    }

    /// Adds or replaces the adapter for its host.
    pub fn register(&self, adapter: Box<dyn HostAdapter>) {
        let host = adapter.host();
        if let Ok(mut map) = self.adapters.write() {
            map.insert(host, Arc::from(adapter));
        }
    }

    /// Returns the adapter for `host`.
    pub fn get(&self, host: Host) -> Arc<dyn HostAdapter> {
        self.adapters
            .read()
            .ok()
            .and_then(|map| map.get(&host).cloned())
            .unwrap_or_else(|| Arc::new(NoopAdapter::new(host)))
    }
}

impl Default for AdapterRegistry {
    fn default() -> Self {
        Self::standard()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn standard_registers_every_host_without_fallback() {
        let registry = AdapterRegistry::standard();
        for host in [
            Host::Claude,
            Host::Codex,
            Host::Agy,
            Host::Cursor,
            Host::Kimi,
            Host::Deepseek,
            Host::Opencode,
            Host::Pi,
        ] {
            let first = registry.get(host);
            assert_eq!(first.host(), host);
            assert!(
                Arc::ptr_eq(&first, &registry.get(host)),
                "fallback for {host:?}"
            );
        }
    }

    #[test]
    fn unknown_hosts_fall_back_to_the_noop_adapter() {
        let registry = AdapterRegistry::standard();
        assert_eq!(registry.get(Host::Claude).host(), Host::Claude);
        assert_eq!(registry.get(Host::Cursor).host(), Host::Cursor);
    }
}
