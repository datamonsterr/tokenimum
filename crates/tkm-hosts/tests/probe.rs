#![forbid(unsafe_code)]
#![allow(clippy::unwrap_used, clippy::expect_used)]
use serde_json::Value;
use std::{
    collections::BTreeSet,
    fs,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};
use tkm_hosts::{
    HostAdapter, InstallContext, claude::ClaudeAdapter, codex::CodexAdapter, probe::expected_events,
};
use tkm_proto::{EventKind, Host};

struct Scratch(PathBuf);
impl Scratch {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "tkm-probe-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }
    fn context(&self) -> InstallContext {
        InstallContext {
            agent_home: self.0.clone(),
            ..InstallContext::default()
        }
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn installed_events(path: &std::path::Path) -> BTreeSet<String> {
    let value: Value = serde_json::from_str(&fs::read_to_string(path).unwrap()).unwrap();
    value["hooks"]
        .as_object()
        .unwrap()
        .keys()
        .cloned()
        .collect()
}

fn expected_names(host: Host) -> BTreeSet<String> {
    expected_events(host)
        .iter()
        .map(|event| event.native.to_owned())
        .collect()
}

#[test]
fn claude_expected_events_match_installed_hooks() {
    let temp = Scratch::new();
    ClaudeAdapter::new().install(&temp.context()).unwrap();
    assert_eq!(
        installed_events(&temp.0.join("settings.json")),
        expected_names(Host::Claude)
    );
    assert!(expected_events(Host::Claude).iter().all(|e| e.required));
}

#[test]
fn codex_expected_events_match_installed_hooks() {
    let temp = Scratch::new();
    CodexAdapter::new().install(&temp.context()).unwrap();
    assert_eq!(
        installed_events(&temp.0.join("hooks.json")),
        expected_names(Host::Codex)
    );
    assert!(expected_events(Host::Codex).iter().all(|e| e.required));
}

#[test]
fn claude_event_kinds_are_normalized() {
    let events = expected_events(Host::Claude);
    let kind = |native: &str| events.iter().find(|e| e.native == native).unwrap().kind;
    assert_eq!(kind("SessionStart"), EventKind::SessionStart);
    assert_eq!(kind("UserPromptSubmit"), EventKind::Prompt);
    assert_eq!(kind("PostToolUse"), EventKind::ToolDone);
    assert_eq!(kind("PostToolUseFailure"), EventKind::ToolDone);
    assert_eq!(kind("Stop"), EventKind::TurnEnd);
    assert_eq!(kind("SessionEnd"), EventKind::SessionEnd);
}

#[test]
fn shim_hosts_have_probe_coverage_for_all_lifecycle_events() {
    for host in [Host::Opencode, Host::Pi] {
        assert_eq!(expected_events(host).len(), 5);
        assert!(
            expected_events(host)
                .iter()
                .any(|e| e.kind == EventKind::SessionEnd)
        );
    }
}

#[test]
fn env_markers_are_empty_for_hosts_without_markers() {
    assert_eq!(tkm_hosts::env_markers(Host::Claude), &[] as &[&str]);
    assert_eq!(tkm_hosts::env_markers(Host::Codex), &[] as &[&str]);
}

#[test]
fn opencode_forwards_its_database_and_data_home_overrides() {
    assert_eq!(
        tkm_hosts::env_markers(Host::Opencode),
        ["OPENCODE_DB", "XDG_DATA_HOME"]
    );
}

#[test]
fn pi_forwards_its_agent_directory_override() {
    assert_eq!(tkm_hosts::env_markers(Host::Pi), ["PI_CODING_AGENT_DIR"]);
}
