//! Cursor hook payload mapping.

use std::path::PathBuf;

use serde_json::Value;
use tkm_core::model::{Call, HookEvent, StopReason};
use tkm_proto::{EventKind, Host, Request};

use crate::writer::invalid;
use crate::{AdapterError, digest};

pub(super) fn map_event(request: &Request) -> Result<Option<HookEvent>, AdapterError> {
    let native = super::native_event(request)?;
    let kind = match native {
        "sessionStart" => EventKind::SessionStart,
        "beforeSubmitPrompt" => EventKind::Prompt,
        "postToolUse" | "postToolUseFailure" => EventKind::ToolDone,
        "stop" => EventKind::TurnEnd,
        "sessionEnd" => EventKind::SessionEnd,
        _ => return Ok(None),
    };

    let payload = &request.payload;

    let session_id = payload
        .get("conversation_id")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .or_else(|| {
            payload
                .get("session_id")
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
        })
        .or_else(|| {
            payload
                .get("generation_id")
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
        })
        .ok_or_else(|| invalid("required session identifier is missing or empty"))?;

    let mut event = HookEvent::new(Host::Cursor, kind, session_id);

    event.turn_id = payload
        .get("generation_id")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_owned);

    event.cwd = payload
        .get("workspace_roots")
        .and_then(Value::as_array)
        .and_then(|roots| roots.first())
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            request
                .cwd
                .as_deref()
                .filter(|s| !s.is_empty())
                .map(PathBuf::from)
        });

    match kind {
        EventKind::Prompt => {
            let prompt_text = payload
                .get("prompt")
                .and_then(Value::as_str)
                .ok_or_else(|| invalid("prompt must be a string"))?;
            event.prompt = Some(prompt_text.to_owned());
        }
        EventKind::ToolDone => {
            let failure = native == "postToolUseFailure";
            let tool = if failure {
                match payload.get("tool_name") {
                    None | Some(Value::Null) => "",
                    Some(Value::String(s)) => s.as_str(),
                    _ => return Err(invalid("tool_name must be a string")),
                }
            } else {
                payload
                    .get("tool_name")
                    .and_then(Value::as_str)
                    .ok_or_else(|| invalid("tool_name must be a string"))?
            };
            let input = payload.get("tool_input").unwrap_or(&Value::Null);
            let duration_ms = parse_duration(payload.get("duration"))?;

            let output_digest = if failure {
                let error_msg = match payload.get("error_message") {
                    None | Some(Value::Null) => "",
                    Some(Value::String(s)) => s.as_str(),
                    _ => return Err(invalid("error_message must be a string")),
                };
                let err_val = Value::String(error_msg.to_owned());
                digest::output_digest(&err_val)
            } else {
                let output = payload.get("tool_output").unwrap_or(&Value::Null);
                digest::output_digest(output)
            };

            event.call = Some(Call {
                tool: tool.to_owned(),
                args_digest: digest::args_digest(tool, input),
                output_digest,
                exit_code: None,
                is_error: failure,
                duration_ms,
                edits: Vec::new(),
            });
        }
        EventKind::TurnEnd => {
            let status = payload.get("status").and_then(Value::as_str);
            let stopped_by = match status {
                Some("completed") => StopReason::OwnStop,
                Some("aborted") => StopReason::Interrupted,
                Some("error") => StopReason::Error,
                _ => StopReason::Unknown,
            };
            event.stopped_by = Some(stopped_by);
        }
        _ => {}
    }

    Ok(Some(event))
}

fn parse_duration(value: Option<&Value>) -> Result<Option<u64>, AdapterError> {
    match value {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Number(n)) => {
            if let Some(val) = n.as_u64() {
                Ok(Some(val))
            } else if let Some(val) = n.as_f64() {
                #[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
                if val >= 0.0 && val < u64::MAX as f64 {
                    Ok(Some(val as u64))
                } else {
                    Ok(None)
                }
            } else {
                Err(invalid("duration must be numeric"))
            }
        }
        _ => Err(invalid("duration must be numeric")),
    }
}
