//! Hook payload to [`HookEvent`] mapping; no I/O.

use std::path::{Path, PathBuf};

use serde_json::{Map, Value};
use tkm_core::model::{Call, HookEvent, StopReason};
use tkm_proto::{EventKind, Host, PROTOCOL_VERSION, Request};

use super::patch;
use crate::AdapterError;
use crate::digest::{args_digest, output_digest};

const SHELL_TOOL: &str = "Bash";
const PATCH_TOOL: &str = "apply_patch";

fn invalid(message: &str) -> AdapterError {
    AdapterError::InvalidPayload {
        message: message.into(),
    }
}

fn kind_of(native: &str) -> Option<EventKind> {
    match native {
        "SessionStart" => Some(EventKind::SessionStart),
        "UserPromptSubmit" => Some(EventKind::Prompt),
        "PostToolUse" => Some(EventKind::ToolDone),
        "Stop" => Some(EventKind::TurnEnd),
        "SessionEnd" => Some(EventKind::SessionEnd),
        _ => None,
    }
}

fn string_field(payload: &Map<String, Value>, key: &str) -> Option<String> {
    payload.get(key).and_then(Value::as_str).map(str::to_owned)
}

pub(crate) fn map_event(request: &Request) -> Result<Option<HookEvent>, AdapterError> {
    if request.version != PROTOCOL_VERSION {
        return Err(invalid("unsupported protocol version"));
    }
    if request.host != Host::Codex {
        return Err(invalid("request is not for the codex host"));
    }
    let payload = request
        .payload
        .as_object()
        .ok_or_else(|| invalid("payload is not a JSON object"))?;
    let session_id = string_field(payload, "session_id")
        .filter(|id| !id.is_empty())
        .ok_or_else(|| invalid("missing session_id"))?;
    let native = super::native_event(request)?.ok_or_else(|| invalid("missing hook_event_name"))?;
    let Some(kind) = kind_of(native) else {
        return Ok(None);
    };

    let cwd = string_field(payload, "cwd").or_else(|| request.cwd.clone());
    let mut event = HookEvent::new(Host::Codex, kind, session_id);
    event.cwd = cwd.map(PathBuf::from);
    match kind {
        EventKind::Prompt => {
            event.turn_id = string_field(payload, "turn_id");
            event.prompt =
                Some(string_field(payload, "prompt").ok_or_else(|| invalid("missing prompt"))?);
        }
        EventKind::ToolDone => {
            event.turn_id = string_field(payload, "turn_id");
            event.call = Some(build_call(payload, event.cwd.as_deref())?);
        }
        EventKind::TurnEnd => {
            event.turn_id = string_field(payload, "turn_id");
            event.stopped_by = Some(StopReason::OwnStop);
        }
        _ => {}
    }
    Ok(Some(event))
}

/// Codex delivers `tool_response` as a JSON string for shell chunks.
fn parsed_response(response: &Value) -> Value {
    if let Value::String(text) = response
        && let Ok(parsed @ Value::Object(_)) = serde_json::from_str::<Value>(text)
    {
        return parsed;
    }
    response.clone()
}

fn build_call(payload: &Map<String, Value>, cwd: Option<&Path>) -> Result<Call, AdapterError> {
    let tool = string_field(payload, "tool_name").ok_or_else(|| invalid("missing tool_name"))?;
    let input = payload.get("tool_input").unwrap_or(&Value::Null);
    let response = parsed_response(payload.get("tool_response").unwrap_or(&Value::Null));
    let shell = tool == SHELL_TOOL;

    let exit_code = if shell {
        response
            .get("exit_code")
            .and_then(Value::as_i64)
            .and_then(|code| i32::try_from(code).ok())
    } else {
        None
    };
    let reported_error = !shell && response.get("isError").and_then(Value::as_bool) == Some(true);
    let changes = if tool == PATCH_TOOL {
        input
            .get("command")
            .and_then(Value::as_str)
            .map_or_else(Vec::new, |text| patch::from_text(text, cwd))
    } else {
        Vec::new()
    };
    let args = if tool == PATCH_TOOL {
        patch::args_digest(&changes)
    } else {
        args_digest(&tool, input)
    };
    Ok(Call {
        args_digest: args,
        output_digest: output_digest(&response),
        exit_code,
        is_error: reported_error || exit_code.is_some_and(|code| code != 0),
        duration_ms: None,
        edits: changes.iter().map(patch::Change::edit).collect(),
        tool,
    })
}
