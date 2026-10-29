use crate::{
    AdapterError, InstallContext, InstallReport,
    writer::{self, HookSpec},
};
use serde_json::json;
use std::path::PathBuf;
use tkm_proto::EventKind;

fn paths(context: &InstallContext) -> (PathBuf, PathBuf) {
    if let Some(repo) = &context.repo_root {
        (repo.clone(), repo.join(".claude/settings.local.json"))
    } else {
        (
            context.agent_home.clone(),
            context.agent_home.join("settings.json"),
        )
    }
}

pub(super) fn install(context: &InstallContext) -> Result<InstallReport, AdapterError> {
    let (root, path) = paths(context);
    let hooks: Vec<_> = [("SessionStart", EventKind::SessionStart), ("UserPromptSubmit", EventKind::Prompt),
        ("PostToolUse", EventKind::ToolDone), ("PostToolUseFailure", EventKind::ToolDone),
        ("Stop", EventKind::TurnEnd), ("SessionEnd", EventKind::SessionEnd)].into_iter().map(|(native, kind)| HookSpec {
            native_event: native.to_owned(), kind,
            matcher: (kind == EventKind::ToolDone).then(|| "*".to_owned()),
            handler: json!({"type":"command","command":"tkm-hook","args":["--host","claude","--event",kind.as_str()],"timeout":if kind == EventKind::Prompt { 3 } else { 1 }}),
        }).collect();
    writer::install_hooks(&root, &path, "claude", &hooks, context.dry_run)
}

pub(super) fn uninstall(context: &InstallContext) -> Result<InstallReport, AdapterError> {
    let (root, path) = paths(context);
    writer::uninstall_hooks(&root, &path, "claude", context.dry_run)
}
