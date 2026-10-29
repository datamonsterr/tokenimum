use tkm_core::{
    model::Timestamp,
    prune::{PruneConfig, PruneEvidence, ToolEvidence, decide},
};
use tkm_proto::Host;

#[test]
fn superorchestrator_tools_are_protected() {
    let config = PruneConfig {
        superorchestrator_adapter_enabled: true,
        ..Default::default()
    };
    let mut evidence = PruneEvidence::default();
    for name in [
        "mcp__superorchestrator__report_progress",
        "mcp__superorchestrator__cancel_agent",
        "mcp__superorchestrator__anything",
    ] {
        evidence.tools.insert(
            name.to_string(),
            ToolEvidence {
                prompts: 20,
                sessions: 5,
                ..Default::default()
            },
        );
    }
    let result = decide(&evidence, Host::Claude, &config, None, Timestamp(0));
    assert!(result.candidates.is_empty());
    assert_eq!(result.protected_skipped.len(), 3);
}
