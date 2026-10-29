//! Antigravity CLI hook installation.

use crate::{AdapterError, InstallContext, InstallReport};
use serde_json::json;
use std::path::PathBuf;

fn target_paths(context: &InstallContext) -> (PathBuf, PathBuf) {
    if let Some(ref repo) = context.repo_root {
        let path = repo.join(".agents").join("hooks.json");
        (repo.clone(), path)
    } else {
        let path = context.agent_home.join("config").join("hooks.json");
        (context.agent_home.clone(), path)
    }
}

pub(super) fn install(context: &InstallContext) -> Result<InstallReport, AdapterError> {
    let (root, path) = target_paths(context);
    let value = json!({
        "SessionStart": [
            {
                "type": "command",
                "command": "tkm-hook --host agy --event session_start",
                "timeout": 1
            }
        ],
        "PreInvocation": [
            {
                "type": "command",
                "command": "tkm-hook --host agy --event prompt",
                "timeout": 3
            }
        ],
        "PostToolUse": [
            {
                "type": "command",
                "command": "tkm-hook --host agy --event tool_done",
                "timeout": 1
            }
        ],
        "Stop": [
            {
                "type": "command",
                "command": "tkm-hook --host agy --event turn_end",
                "timeout": 1
            }
        ]
    });
    let mut report = crate::writer::install_member(&root, &path, "tokenimum", &value, context.dry_run)?;
    report.instructions.clear();
    Ok(report)
}

pub(super) fn uninstall(context: &InstallContext) -> Result<InstallReport, AdapterError> {
    let (root, path) = target_paths(context);
    let mut report = crate::writer::uninstall_member(&root, &path, "tokenimum", context.dry_run)?;
    report.instructions.clear();
    Ok(report)
}
