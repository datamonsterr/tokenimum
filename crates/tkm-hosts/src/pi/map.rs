use crate::{AdapterError, digest, shim, writer::invalid};
use serde_json::Value;
use std::path::{Path, PathBuf};
use tkm_core::model::{Call, EditKind, EditVia, FileEdit, HookEvent};
use tkm_proto::{EventKind, Host, Request};

pub(super) fn map_event(request: &Request) -> Result<Option<HookEvent>, AdapterError> {
    let hook = super::native(request)?;
    // `agent_end` ends the run for one prompt; `turn_end` fires per model
    // response and is ignored (ADR 0025).
    let kind = match hook {
        "session_start" => EventKind::SessionStart,
        "before_agent_start" => EventKind::Prompt,
        "tool_result" => EventKind::ToolDone,
        "agent_end" => EventKind::TurnEnd,
        "session_shutdown" => EventKind::SessionEnd,
        _ => return Ok(None),
    };
    let payload = &request.payload;
    let session_id = payload
        .get("session_id")
        .and_then(Value::as_str)
        .ok_or_else(|| invalid("required payload string is missing or invalid"))?;
    let mut event = HookEvent::new(Host::Pi, kind, session_id);
    event.cwd = payload
        .get("cwd")
        .and_then(Value::as_str)
        .or(request.cwd.as_deref())
        .map(PathBuf::from);
    let data = payload.get("event").unwrap_or(&Value::Null);
    match kind {
        EventKind::Prompt => {
            // A run started by a custom message has no person prompt.
            if data.get("messageType").and_then(Value::as_str) == Some("custom") {
                return Ok(None);
            }
            let prompt = data
                .get("prompt")
                .and_then(Value::as_str)
                .ok_or_else(|| invalid("prompt must be a string"))?;
            event.prompt = Some(prompt.to_owned());
        }
        EventKind::ToolDone => event.call = Some(call(data, event.cwd.as_deref())?),
        _ => {}
    }
    Ok(Some(event))
}

fn call(data: &Value, cwd: Option<&Path>) -> Result<Call, AdapterError> {
    let tool = data
        .get("toolName")
        .and_then(Value::as_str)
        .ok_or_else(|| invalid("required payload string is missing or invalid"))?;
    let input = data
        .get("input")
        .filter(|input| input.is_object())
        .ok_or_else(|| invalid("tool input must be an object"))?;
    Ok(Call {
        tool: tool.to_owned(),
        args_digest: digest::args_digest(tool, input),
        output_digest: digest::output_digest(data.get("content").unwrap_or(&Value::Null)),
        is_error: data.get("isError").and_then(Value::as_bool) == Some(true),
        edits: if data.get("isError").and_then(Value::as_bool) == Some(true) {
            Vec::new()
        } else {
            edits(tool, input, cwd)
        },
        ..Call::default()
    })
}

/// File edits of Pi's `write` and `edit` tools, relative to the session
/// directory. Pi does not say whether `write` created the file.
pub(super) fn edits(tool: &str, input: &Value, cwd: Option<&Path>) -> Vec<FileEdit> {
    if !matches!(tool, "write" | "edit") {
        return Vec::new();
    }
    let Some(cwd) = cwd else {
        return Vec::new();
    };
    ["path", "file_path", "filePath"]
        .iter()
        .find_map(|key| input.get(*key).and_then(Value::as_str))
        .and_then(|file| shim::repo_relative(cwd, file))
        .map(|path| FileEdit {
            path,
            kind: EditKind::Modify,
            via: EditVia::Tool,
        })
        .into_iter()
        .collect()
}
