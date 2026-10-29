#![forbid(unsafe_code)]
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::{Value, json};
use tkm_hosts::cursor::CursorAdapter;
use tkm_hosts::{HostAdapter, InstallContext};

struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "tkm-cursor-install-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn user_context(&self) -> InstallContext {
        let cursor_home = self.0.join(".cursor");
        fs::create_dir_all(&cursor_home).unwrap();
        InstallContext {
            agent_home: cursor_home,
            repo_root: None,
            dry_run: false,
        }
    }

    fn project_context(&self) -> InstallContext {
        let cursor_home = self.0.join(".cursor");
        let repo_root = self.0.join("repo");
        fs::create_dir_all(&cursor_home).unwrap();
        fs::create_dir_all(&repo_root).unwrap();
        InstallContext {
            agent_home: cursor_home,
            repo_root: Some(repo_root),
            dry_run: false,
        }
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// Deleting only inserted bytes from `changed` must recover `original`.
fn assert_foreign_bytes_preserved(original: &[u8], changed: &[u8]) {
    let mut cursor = 0;
    for &byte in changed {
        if original.get(cursor) == Some(&byte) {
            cursor += 1;
        }
    }
    assert_eq!(cursor, original.len());
}

#[test]
fn foreign_handler_in_same_event_and_foreign_keys_kept_byte_for_byte() {
    let scratch = Scratch::new();
    let context = scratch.user_context();
    let hooks_path = context.agent_home.join("hooks.json");
    let backup_path = context.agent_home.join("hooks.json.tkm-bak");

    let original = br#"{
  "version": 1,
  "foreign_key": "custom_val",
  "hooks": {
    "sessionStart": [
      {
        "command": "./foreign-script.sh",
        "timeout": 5
      }
    ]
  }
}"#;
    fs::write(&hooks_path, original).unwrap();

    let adapter = CursorAdapter::new();
    let report = adapter.install(&context).unwrap();
    assert_eq!(
        report.changed_files,
        vec![hooks_path.clone(), backup_path.clone()]
    );
    assert_eq!(fs::read(&backup_path).unwrap(), original);

    let installed_bytes = fs::read(&hooks_path).unwrap();
    assert_foreign_bytes_preserved(original, &installed_bytes);

    let installed_json: Value = serde_json::from_slice(&installed_bytes).unwrap();
    assert_eq!(installed_json["foreign_key"], "custom_val");

    // Foreign handler preserved
    let session_start_hooks = installed_json["hooks"]["sessionStart"].as_array().unwrap();
    assert!(
        session_start_hooks
            .iter()
            .any(|h| h["command"] == "./foreign-script.sh")
    );

    // tokenimum handlers installed
    for (event, kind, timeout) in [
        ("sessionStart", "session_start", 1),
        ("beforeSubmitPrompt", "prompt", 3),
        ("postToolUse", "tool_done", 1),
        ("postToolUseFailure", "tool_done", 1),
        ("stop", "turn_end", 1),
        ("sessionEnd", "session_end", 1),
    ] {
        let event_hooks = installed_json["hooks"][event].as_array().unwrap();
        let expected_cmd = format!("tkm-hook --host cursor --event {kind}");
        let own = event_hooks
            .iter()
            .find(|h| h["command"] == expected_cmd)
            .expect("tokenimum hook present");
        assert_eq!(own["timeout"], timeout);
    }

    // Idempotent install
    let report2 = adapter.install(&context).unwrap();
    assert_eq!(report2.changed_files.len(), 0);
    assert_eq!(fs::read(&hooks_path).unwrap(), installed_bytes);

    // Uninstall restores exact original bytes
    let uninstall_report = adapter.uninstall(&context).unwrap();
    assert_eq!(
        uninstall_report.changed_files,
        vec![hooks_path.clone(), backup_path.clone()]
    );
    assert_eq!(fs::read(&hooks_path).unwrap(), original);
    assert!(!backup_path.exists());

    // Idempotent uninstall
    let uninstall_report2 = adapter.uninstall(&context).unwrap();
    assert_eq!(uninstall_report2.changed_files.len(), 0);
}

#[test]
fn version_seeded_only_when_absent() {
    let scratch = Scratch::new();
    let context = scratch.user_context();
    let hooks_path = context.agent_home.join("hooks.json");

    // Case 1: absent version is seeded to 1
    fs::write(&hooks_path, b"{\"foreign\":\"value\"}").unwrap();
    let adapter = CursorAdapter::new();
    adapter.install(&context).unwrap();
    let val: Value = serde_json::from_slice(&fs::read(&hooks_path).unwrap()).unwrap();
    assert_eq!(val["version"], 1);

    // Case 2: existing version (e.g. 2) is not overwritten
    let scratch2 = Scratch::new();
    let context2 = scratch2.user_context();
    let hooks_path2 = context2.agent_home.join("hooks.json");
    fs::write(&hooks_path2, b"{\"version\":2}").unwrap();
    adapter.install(&context2).unwrap();
    let val2: Value = serde_json::from_slice(&fs::read(&hooks_path2).unwrap()).unwrap();
    assert_eq!(val2["version"], 2);
}

#[test]
fn dry_run_does_not_modify_disk() {
    let scratch = Scratch::new();
    let mut context = scratch.user_context();
    context.dry_run = true;
    let hooks_path = context.agent_home.join("hooks.json");

    let adapter = CursorAdapter::new();
    let report = adapter.install(&context).unwrap();
    assert!(report.dry_run);
    assert_eq!(
        report.changed_files,
        vec![
            hooks_path.clone(),
            context.agent_home.join("hooks.json.tkm-bak")
        ]
    );
    assert!(!hooks_path.exists());
}

#[test]
fn created_file_removed_on_uninstall() {
    let scratch = Scratch::new();
    let context = scratch.user_context();
    let hooks_path = context.agent_home.join("hooks.json");
    assert!(!hooks_path.exists());

    let adapter = CursorAdapter::new();
    adapter.install(&context).unwrap();
    assert!(hooks_path.exists());

    adapter.uninstall(&context).unwrap();
    assert!(!hooks_path.exists());
}

#[test]
fn claude_double_registration_instruction_appears_when_claude_settings_holds_tokenimum_hook() {
    let scratch = Scratch::new();
    let context = scratch.user_context();
    // In user scope, agent_home is scratch.0/.cursor, parent is scratch.0
    let claude_settings = scratch.0.join(".claude").join("settings.json");
    fs::create_dir_all(claude_settings.parent().unwrap()).unwrap();

    let adapter = CursorAdapter::new();

    // 1. Without claude tokenimum handler: no instruction
    fs::write(
        &claude_settings,
        b"{\"hooks\":{\"UserPromptSubmit\":[{\"matcher\":\"*\",\"hooks\":[{\"command\":\"other-cmd\"}]}]}}",
    )
    .unwrap();
    let report_clean = adapter.install(&context).unwrap();
    assert_eq!(report_clean.instructions, Vec::<String>::new());

    // 2. With claude tokenimum handler: instruction appears
    let claude_with_tokenimum = json!({
        "hooks": {
            "UserPromptSubmit": [
                {
                    "matcher": "*",
                    "hooks": [
                        {
                            "type": "command",
                            "command": "tkm-hook",
                            "args": ["--host", "claude", "--event", "prompt"],
                            "timeout": 3
                        }
                    ]
                }
            ]
        }
    });
    fs::write(
        &claude_settings,
        serde_json::to_vec_pretty(&claude_with_tokenimum).unwrap(),
    )
    .unwrap();

    let report_warn = adapter.install(&context).unwrap();
    assert_eq!(report_warn.instructions.len(), 1);
    let msg = &report_warn.instructions[0];
    assert!(
        msg.contains("Cursor also runs Claude Code hooks from"),
        "actual: {msg}"
    );
    assert!(
        msg.contains(&claude_settings.display().to_string()),
        "actual: {msg}"
    );
    assert!(
        msg.contains("(Settings > Agents > Third-Party Imports). tokenimum ignores the duplicate Claude-host events from Cursor; turn that setting off to avoid running the hook twice."),
        "actual: {msg}"
    );

    // Project scope checks: .claude/settings.local.json
    let project_scratch = Scratch::new();
    let project_context = project_scratch.project_context();
    let repo_root = project_context.repo_root.as_ref().unwrap();
    let repo_claude_local = repo_root.join(".claude").join("settings.local.json");
    fs::create_dir_all(repo_claude_local.parent().unwrap()).unwrap();
    fs::write(
        &repo_claude_local,
        serde_json::to_vec_pretty(&claude_with_tokenimum).unwrap(),
    )
    .unwrap();

    let project_report = adapter.install(&project_context).unwrap();
    assert_eq!(project_report.instructions.len(), 1);
    assert!(project_report.instructions[0].contains(&repo_claude_local.display().to_string()));

    // Malformed claude settings file yields no error and no instruction
    fs::write(&repo_claude_local, b"not valid json").unwrap();
    let malformed_report = adapter.install(&project_context).unwrap();
    assert_eq!(malformed_report.instructions, Vec::<String>::new());
}
