//! Cursor hook registration in `hooks.json` (ADR 0020).

use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};
use tkm_proto::EventKind;

use crate::writer::{self, HookSpec};
use crate::{AdapterError, InstallContext, InstallReport};

const HOST: &str = "cursor";
const CONTAINER: &str = "hooks";

const EVENTS: [(&str, EventKind); 6] = [
    ("sessionStart", EventKind::SessionStart),
    ("beforeSubmitPrompt", EventKind::Prompt),
    ("postToolUse", EventKind::ToolDone),
    ("postToolUseFailure", EventKind::ToolDone),
    ("stop", EventKind::TurnEnd),
    ("sessionEnd", EventKind::SessionEnd),
];

fn paths(context: &InstallContext) -> (PathBuf, PathBuf) {
    match &context.repo_root {
        Some(repo) => (repo.clone(), repo.join(".cursor/hooks.json")),
        None => (
            context.agent_home.clone(),
            context.agent_home.join("hooks.json"),
        ),
    }
}

fn specs() -> Vec<HookSpec> {
    EVENTS
        .into_iter()
        .map(|(native, kind)| {
            let timeout = if kind == EventKind::Prompt { 3 } else { 1 };
            HookSpec {
                native_event: native.to_owned(),
                kind,
                matcher: None,
                handler: json!({
                    "command": format!("tkm-hook --host {HOST} --event {}", kind.as_str()),
                    "timeout": timeout,
                }),
            }
        })
        .collect()
}

fn claude_has_tokenimum_hooks(path: &Path) -> bool {
    let Ok(contents) = fs::read(path) else {
        return false;
    };
    let Ok(json) = serde_json::from_slice::<Value>(&contents) else {
        return false;
    };
    let Some(hooks_obj) = json.get("hooks").and_then(Value::as_object) else {
        return false;
    };
    for groups in hooks_obj.values() {
        let Some(groups_arr) = groups.as_array() else {
            continue;
        };
        for group in groups_arr {
            let Some(handlers) = group.get("hooks").and_then(Value::as_array) else {
                continue;
            };
            for handler in handlers {
                if writer::owns_handler(handler, "claude") {
                    return true;
                }
            }
        }
    }
    false
}

fn check_claude_double_registration(context: &InstallContext, instructions: &mut Vec<String>) {
    let claude_files = match &context.repo_root {
        Some(repo) => vec![
            repo.join(".claude/settings.json"),
            repo.join(".claude/settings.local.json"),
        ],
        None => match context.agent_home.parent() {
            Some(parent) => vec![parent.join(".claude/settings.json")],
            None => Vec::new(),
        },
    };

    for claude_path in claude_files {
        if claude_has_tokenimum_hooks(&claude_path) {
            instructions.push(format!(
                "Cursor also runs Claude Code hooks from {} (Settings > Agents > Third-Party Imports). tokenimum ignores the duplicate Claude-host events from Cursor; turn that setting off to avoid running the hook twice.",
                claude_path.display()
            ));
        }
    }
}

pub(super) fn install(context: &InstallContext) -> Result<InstallReport, AdapterError> {
    let (root, path) = paths(context);
    let seed = [("version", json!(1))];
    let mut report = writer::install_flat_hooks(
        &root,
        &path,
        HOST,
        CONTAINER,
        &seed,
        &specs(),
        context.dry_run,
    )?;
    check_claude_double_registration(context, &mut report.instructions);
    Ok(report)
}

pub(super) fn uninstall(context: &InstallContext) -> Result<InstallReport, AdapterError> {
    let (root, path) = paths(context);
    writer::uninstall_flat_hooks(&root, &path, HOST, CONTAINER, &["version"], context.dry_run)
}

/// Verified project-local pruning settings path.
pub(crate) const DENY_PATH: &str = ".cursor/cli.json";
