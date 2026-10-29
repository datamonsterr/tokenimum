use std::path::PathBuf;

use serde_json::Value;
use tkm_core::model::{Call, HookEvent, StopReason, Usage};
use tkm_proto::{EventKind, Host, Request};

use crate::AdapterError;

pub(super) fn map_event(request: &Request) -> Result<Option<HookEvent>, AdapterError> {
    let native = super::native_event(request)?;

    let session_id = request
        .env
        .get("DEEPSEEK_SESSION_ID")
        .or_else(|| request.env.get("CODEWHALE_SESSION_ID"))
        .map(String::as_str)
        .or_else(|| {
            request
                .payload
                .as_object()
                .and_then(|m| m.get("session_id"))
                .and_then(Value::as_str)
        });

    let Some(session_id) = session_id.filter(|s| !s.is_empty()) else {
        return Err(AdapterError::InvalidPayload {
            message: "session_id is missing or empty".to_owned(),
        });
    };

    let cwd = request
        .env
        .get("DEEPSEEK_WORKSPACE")
        .map(String::as_str)
        .or_else(|| {
            request
                .payload
                .as_object()
                .and_then(|m| m.get("workspace"))
                .and_then(Value::as_str)
        })
        .or(request.cwd.as_deref())
        .map(PathBuf::from);

    match native {
        "session_start" => {
            let mut event = HookEvent::new(Host::Deepseek, EventKind::SessionStart, session_id);
            event.cwd = cwd;
            Ok(Some(event))
        }
        "message_submit" => {
            let mut event = HookEvent::new(Host::Deepseek, EventKind::Prompt, session_id);
            event.cwd = cwd;
            event.prompt = request
                .payload
                .as_object()
                .and_then(|m| m.get("text"))
                .and_then(Value::as_str)
                .map(str::to_owned);
            Ok(Some(event))
        }
        "tool_call_before" => {
            let mut event = HookEvent::new(Host::Deepseek, EventKind::ToolBefore, session_id);
            event.cwd = cwd;
            let (tool, args_val) = extract_tool_and_args(request);
            let args_digest = compute_args_digest(&tool, &args_val);
            event.call = Some(Call {
                tool,
                args_digest,
                ..Call::default()
            });
            Ok(Some(event))
        }
        "tool_call_after" => {
            let mut event = HookEvent::new(Host::Deepseek, EventKind::ToolDone, session_id);
            event.cwd = cwd;
            let (tool, args_val) = extract_tool_and_args(request);
            let mut args_digest = compute_args_digest(&tool, &args_val);

            let receipt = request
                .payload
                .as_object()
                .and_then(|m| m.get("execution_receipt"))
                .filter(|v| v.is_object());

            if let Some(rec) = receipt {
                if rec.get("command").is_some() && is_shell_tool(&tool) {
                    args_digest = crate::digest::args_digest("bash", rec);
                }
            }

            let exit_code = receipt
                .and_then(|r| r.get("exit_code"))
                .and_then(Value::as_i64)
                .and_then(|c| i32::try_from(c).ok())
                .or_else(|| {
                    request
                        .env
                        .get("DEEPSEEK_TOOL_EXIT_CODE")
                        .and_then(|s| s.parse::<i32>().ok())
                });

            let output_digest = if let Some(rec) = receipt {
                crate::digest::output_digest(rec)
            } else if let Some(res) = request.env.get("DEEPSEEK_TOOL_RESULT") {
                crate::digest::output_digest(&Value::String(res.clone()))
            } else if let Some(out) = request
                .payload
                .as_object()
                .and_then(|m| m.get("result").or_else(|| m.get("output")))
            {
                crate::digest::output_digest(out)
            } else {
                String::new()
            };

            let is_error = if let Some(success) = request.env.get("DEEPSEEK_TOOL_SUCCESS") {
                success == "false"
            } else if let Some(code) = exit_code {
                code != 0
            } else if let Some(status) = request.env.get("DEEPSEEK_TOOL_STATUS") {
                status == "failed" || status == "error"
            } else {
                false
            };

            event.call = Some(Call {
                tool,
                args_digest,
                output_digest,
                exit_code,
                is_error,
                ..Call::default()
            });
            Ok(Some(event))
        }
        "turn_end" => {
            let mut event = HookEvent::new(Host::Deepseek, EventKind::TurnEnd, session_id);
            event.cwd = cwd;
            event.turn_id = request
                .payload
                .as_object()
                .and_then(|m| m.get("turn_id"))
                .and_then(Value::as_str)
                .map(str::to_owned);

            event.stopped_by = request
                .payload
                .as_object()
                .and_then(|m| m.get("status"))
                .and_then(Value::as_str)
                .map(|status| match status.to_ascii_lowercase().as_str() {
                    "completed" | "success" | "done" => StopReason::OwnStop,
                    "failed" | "error" => StopReason::Error,
                    "interrupted" | "cancelled" | "canceled" => StopReason::Interrupted,
                    _ => StopReason::Unknown,
                });

            if let Some(usage_obj) = request
                .payload
                .as_object()
                .and_then(|m| m.get("usage"))
                .and_then(Value::as_object)
            {
                event.usage = Usage {
                    input: usage_obj
                        .get("input_tokens")
                        .and_then(Value::as_u64)
                        .unwrap_or(0),
                    output: usage_obj
                        .get("output_tokens")
                        .and_then(Value::as_u64)
                        .unwrap_or(0),
                    cache_read: usage_obj
                        .get("prompt_cache_hit_tokens")
                        .and_then(Value::as_u64)
                        .unwrap_or(0),
                    cache_write: usage_obj
                        .get("prompt_cache_write_tokens")
                        .and_then(Value::as_u64)
                        .unwrap_or(0),
                };
            }
            Ok(Some(event))
        }
        "session_end" => {
            let mut event = HookEvent::new(Host::Deepseek, EventKind::SessionEnd, session_id);
            event.cwd = cwd;
            Ok(Some(event))
        }
        _ => Ok(None),
    }
}

fn is_shell_tool(tool: &str) -> bool {
    matches!(
        tool,
        "exec_shell" | "bash" | "Bash" | "shell" | "shell_command" | "exec_command"
    )
}

fn extract_tool_and_args(request: &Request) -> (String, Value) {
    let tool = request
        .env
        .get("DEEPSEEK_TOOL_NAME")
        .cloned()
        .or_else(|| {
            request
                .payload
                .as_object()
                .and_then(|m| m.get("tool").or_else(|| m.get("tool_name")))
                .and_then(Value::as_str)
                .map(str::to_owned)
        })
        .unwrap_or_default();

    let args_val = if let Some(raw) = request.env.get("DEEPSEEK_TOOL_ARGS") {
        serde_json::from_str::<Value>(raw).unwrap_or_else(|_| Value::String(raw.clone()))
    } else if let Some(payload_args) = request
        .payload
        .as_object()
        .and_then(|m| m.get("args").or_else(|| m.get("tool_args")))
    {
        payload_args.clone()
    } else {
        Value::Null
    };

    (tool, args_val)
}

fn compute_args_digest(tool: &str, args_val: &Value) -> String {
    if is_shell_tool(tool) {
        if let Value::String(s) = args_val {
            crate::digest::truncate(s, 512)
        } else {
            crate::digest::args_digest("bash", args_val)
        }
    } else {
        crate::digest::args_digest(tool, args_val)
    }
}
