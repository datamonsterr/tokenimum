//! Antigravity CLI payload mapping.

use crate::{AdapterError, digest};
use serde_json::Value;
use std::path::{Path, PathBuf};
use tkm_core::model::{Call, EditKind, EditVia, FileEdit, HookEvent, StopReason};
use tkm_proto::{EventKind, Host, Request};

pub(super) fn map_event(request: &Request) -> Result<Option<HookEvent>, AdapterError> {
    let native = super::native_event(request)?;
    let payload = &request.payload;

    let session_id = payload
        .get("conversationId")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .or_else(|| {
            request
                .env
                .get("ANTIGRAVITY_CONVERSATION_ID")
                .filter(|s| !s.is_empty())
                .cloned()
        })
        .ok_or_else(|| {
            crate::writer::invalid("conversationId or ANTIGRAVITY_CONVERSATION_ID missing")
        })?;

    let cwd = payload
        .get("workspacePaths")
        .and_then(Value::as_array)
        .and_then(|arr| arr.first())
        .and_then(Value::as_str)
        .map(PathBuf::from)
        .or_else(|| request.cwd.as_deref().map(PathBuf::from));

    match native {
        "SessionStart" => {
            let mut event = HookEvent::new(Host::Agy, EventKind::SessionStart, session_id);
            event.cwd = cwd;
            event.prompt = payload
                .get("lastUserInput")
                .and_then(Value::as_str)
                .map(str::to_owned);
            Ok(Some(event))
        }
        "PreInvocation" => {
            let invocation_num = payload
                .get("invocationNum")
                .and_then(Value::as_i64)
                .unwrap_or(0);
            if invocation_num > 0 {
                return Ok(None);
            }
            let mut event = HookEvent::new(Host::Agy, EventKind::Prompt, session_id);
            event.cwd = cwd;
            event.prompt = payload
                .get("lastUserInput")
                .and_then(Value::as_str)
                .map(str::to_owned);
            Ok(Some(event))
        }
        "PostToolUse" => {
            let mut event = HookEvent::new(Host::Agy, EventKind::ToolDone, session_id);
            event.cwd = cwd;
            let tool_call = payload.get("toolCall").and_then(Value::as_object);
            let tool_name = tool_call
                .and_then(|tc| tc.get("name"))
                .and_then(Value::as_str)
                .unwrap_or("");
            let args = tool_call
                .and_then(|tc| tc.get("args"))
                .unwrap_or(&Value::Null);
            let error_str = payload.get("error").and_then(Value::as_str).unwrap_or("");
            let is_error = !error_str.is_empty();
            let args_digest = call_args_digest(tool_name, args);
            let output_digest = if is_error {
                digest::output_digest(&Value::String(error_str.to_owned()))
            } else {
                String::new()
            };
            let edits = edits(tool_name, args, event.cwd.as_deref());
            event.call = Some(Call {
                tool: tool_name.to_owned(),
                args_digest,
                output_digest,
                exit_code: None,
                is_error,
                duration_ms: None,
                edits,
            });
            Ok(Some(event))
        }
        "Stop" => {
            let mut event = HookEvent::new(Host::Agy, EventKind::TurnEnd, session_id);
            event.cwd = cwd;
            let error_str = payload.get("error").and_then(Value::as_str).unwrap_or("");
            event.stopped_by = Some(if error_str.is_empty() {
                StopReason::OwnStop
            } else {
                StopReason::Error
            });
            Ok(Some(event))
        }
        _ => Ok(None),
    }
}

pub(super) fn call_args_digest(tool: &str, args: &Value) -> String {
    if tool == "run_command"
        && let Some(cmd) = args.get("CommandLine").and_then(Value::as_str)
    {
        return digest::truncate(cmd, 512);
    }
    digest::args_digest(tool, args)
}

pub(super) fn edits(tool: &str, args: &Value, cwd: Option<&Path>) -> Vec<FileEdit> {
    let kind = match tool {
        "write_file" | "write_to_file" => EditKind::Create,
        "edit_file" | "replace_file_content" | "multi_replace_file_content" => EditKind::Modify,
        _ => return Vec::new(),
    };
    let Some(file_str) = args
        .get("Path")
        .or_else(|| args.get("TargetFile"))
        .or_else(|| args.get("path"))
        .or_else(|| args.get("target_file"))
        .and_then(Value::as_str)
    else {
        return Vec::new();
    };
    let Some(rel_path) = super::repo_relative(file_str, cwd) else {
        return Vec::new();
    };
    vec![FileEdit {
        path: rel_path,
        kind,
        via: EditVia::Tool,
    }]
}
