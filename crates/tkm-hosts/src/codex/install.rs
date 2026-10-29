//! Codex hook registration in `hooks.json` (ADR 0011); `config.toml` is never touched.

use std::path::{Path, PathBuf};

use serde_json::{Value, json};
use tkm_proto::EventKind;

use crate::writer::{self, HookSpec};
use crate::{AdapterError, InstallContext, InstallReport};

const HOST: &str = "codex";
/// Approximate tokens Codex accepts as hook context (its documented default).
const CONTEXT_LIMIT: u64 = 2_500;
const EVENTS: [(&str, EventKind); 5] = [
    ("SessionStart", EventKind::SessionStart),
    ("UserPromptSubmit", EventKind::Prompt),
    ("PostToolUse", EventKind::ToolDone),
    ("Stop", EventKind::TurnEnd),
    ("SessionEnd", EventKind::SessionEnd),
];

/// Writer root and `hooks.json` path for the requested scope.
fn paths(context: &InstallContext) -> (PathBuf, PathBuf) {
    match &context.repo_root {
        Some(repo) => (repo.clone(), repo.join(".codex/hooks.json")),
        None => (
            context.agent_home.clone(),
            context.agent_home.join("hooks.json"),
        ),
    }
}

fn handler(kind: EventKind) -> Value {
    let timeout = if kind == EventKind::Prompt { 3 } else { 1 };
    let mut handler = json!({
        "type": "command",
        "command": format!("tkm-hook --host {HOST} --event {}", kind.as_str()),
        "timeout": timeout,
    });
    // Codex warns when the limit is set on an event that cannot produce context.
    if matches!(
        kind,
        EventKind::SessionStart | EventKind::Prompt | EventKind::ToolDone
    ) {
        handler["additionalContextLimit"] = json!(CONTEXT_LIMIT);
    }
    handler
}

fn specs() -> Vec<HookSpec> {
    EVENTS
        .into_iter()
        .map(|(native, kind)| HookSpec {
            native_event: native.to_owned(),
            kind,
            matcher: (kind == EventKind::ToolDone).then(|| "*".to_owned()),
            handler: handler(kind),
        })
        .collect()
}

/// Codex skips hooks until the person trusts them; tokenimum never writes trust state.
fn trust_instructions(path: &Path) -> Vec<String> {
    vec![format!(
        "Run `codex`, open `/hooks`, then review and trust the tokenimum entries in {}.",
        path.display()
    )]
}

pub(super) fn install(context: &InstallContext) -> Result<InstallReport, AdapterError> {
    let (root, path) = paths(context);
    let mut report = writer::install_hooks(&root, &path, HOST, &specs(), context.dry_run)?;
    report.instructions = trust_instructions(&path);
    Ok(report)
}

pub(super) fn uninstall(context: &InstallContext) -> Result<InstallReport, AdapterError> {
    let (root, path) = paths(context);
    writer::uninstall_hooks(&root, &path, HOST, context.dry_run)
}
