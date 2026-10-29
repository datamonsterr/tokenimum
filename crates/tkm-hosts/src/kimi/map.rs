//! Hook payload mapping for Kimi Code.

use std::path::PathBuf;

use serde_json::Value;
use tkm_core::model::{Call, HookEvent, StopReason};
use tkm_proto::{EventKind, Host, Request};

use crate::digest::{args_digest, output_digest};
use crate::{AdapterError, writer::invalid};

pub(super) fn map_event(request: &Request) -> Result<Option<HookEvent>, AdapterError> {
    let native = super::native_event(request)?;

    let kind = match native {
        "SessionStart" => EventKind::SessionStart,
        "UserPromptSubmit" => EventKind::Prompt,
        "PostToolUse" | "PostToolUseFailure" => EventKind::ToolDone,
        "Stop" => EventKind::TurnEnd,
        "SessionEnd" => EventKind::SessionEnd,
        _ => return Ok(None),
    };

    let payload = request
        .payload
        .as_object()
        .ok_or_else(|| invalid("payload must be an object"))?;

    let session_id = payload
        .get("session_id")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| invalid("required payload string is missing or invalid"))?;

    let mut event = HookEvent::new(Host::Kimi, kind, session_id);
    event.cwd = payload
        .get("cwd")
        .and_then(Value::as_str)
        .or(request.cwd.as_deref())
        .map(PathBuf::from);

    match kind {
        EventKind::Prompt => {
            let prompt = payload
                .get("prompt")
                .and_then(Value::as_str)
                .ok_or_else(|| invalid("missing prompt"))?;
            event.prompt = Some(prompt.to_owned());
        }
        EventKind::ToolDone => {
            let tool = payload
                .get("tool_name")
                .and_then(Value::as_str)
                .ok_or_else(|| invalid("missing tool_name"))?;
            let input = payload.get("tool_input").unwrap_or(&Value::Null);
            let args = args_digest(tool, input);
            let is_failure = native == "PostToolUseFailure";
            let output = if is_failure {
                payload.get("error").unwrap_or(&Value::Null)
            } else {
                payload.get("tool_output").unwrap_or(&Value::Null)
            };

            event.call = Some(Call {
                tool: tool.to_owned(),
                args_digest: args,
                output_digest: output_digest(output),
                exit_code: None,
                is_error: is_failure,
                duration_ms: None,
                edits: Vec::new(),
            });
        }
        EventKind::TurnEnd => {
            event.stopped_by = Some(StopReason::OwnStop);
        }
        _ => {}
    }

    Ok(Some(event))
}
