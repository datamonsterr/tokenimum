#![forbid(unsafe_code)]
use tkm_core::{
    model::Host,
    prune::{PruneConfig, is_protected},
};

#[test]
fn all_protection_sources_and_host_names_are_respected() {
    let mut config = PruneConfig {
        protect: vec!["custom".into(), "private_*".into()],
        ..Default::default()
    };
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
        for tool in [
            "Skill",
            "ToolSearch",
            "Agent",
            "Task",
            "Bash",
            "AskUserQuestion",
            "mcp__headroom__status",
            "custom",
            "private_a",
        ] {
            assert!(is_protected(tool, host, &config), "{host:?}: {tool}");
        }
    }
    for (host, tools) in [
        (Host::Opencode, ["skill", "task", "headroom_status"]),
        (Host::Pi, ["read", "bash", "write"]),
        (Host::Cursor, ["Mcp(headroom:status)", "Read", "Shell"]),
    ] {
        for tool in tools {
            assert!(is_protected(tool, host, &config), "{host:?}: {tool}");
        }
    }
    for (host, tool) in [
        (Host::Claude, "mcp__superorchestrator__spawn_agent"),
        (Host::Opencode, "superorchestrator_spawn_agent"),
        (Host::Cursor, "Mcp(superorchestrator:spawn_agent)"),
    ] {
        assert!(!is_protected(tool, host, &config));
        config.superorchestrator_adapter_enabled = true;
        assert!(is_protected(tool, host, &config));
        config.superorchestrator_adapter_enabled = false;
    }
    assert!(!is_protected("custom_extra", Host::Claude, &config));
    assert!(!is_protected("ordinary", Host::Pi, &config));
}
