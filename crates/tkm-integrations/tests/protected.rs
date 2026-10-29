use tkm_core::prune::{is_protected, PruneConfig};
use tkm_proto::Host;

#[test]
fn superorchestrator_tools_are_protected() {
    let config = PruneConfig { superorchestrator_adapter_enabled: true, ..Default::default() };
    for name in ["mcp__superorchestrator__report_progress", "mcp__superorchestrator__cancel_agent", "mcp__superorchestrator__anything"] {
        assert!(is_protected(name, Host::Claude, &config));
    }
}
