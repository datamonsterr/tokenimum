use crate::{AdapterError, digest, writer::invalid};
use serde_json::Value;
use std::path::{Component, Path, PathBuf};
use tkm_core::model::{Call, EditKind, EditVia, FileEdit, HookEvent};
use tkm_proto::{EventKind, Host, PROTOCOL_VERSION, Request};

pub(super) fn native_event(request: &Request) -> Result<&str, AdapterError> {
    if request.version != PROTOCOL_VERSION {
        return Err(invalid("unsupported protocol version"));
    }
    if request.host != Host::Claude {
        return Err(invalid("host does not match Claude"));
    }
    if !request.payload.is_object() {
        return Err(invalid("payload must be an object"));
    }
    string(&request.payload, "session_id")?;
    if let Some(event) = request.payload.get("hook_event_name") {
        event
            .as_str()
            .ok_or_else(|| invalid("hook_event_name must be a string"))
    } else {
        request
            .host_event
            .as_deref()
            .ok_or_else(|| invalid("native event is missing"))
    }
}

fn string<'a>(value: &'a Value, field: &str) -> Result<&'a str, AdapterError> {
    value
        .get(field)
        .and_then(Value::as_str)
        .ok_or_else(|| invalid("required payload string is missing or invalid"))
}

fn optional_string<'a>(value: &'a Value, field: &str) -> Result<Option<&'a str>, AdapterError> {
    match value.get(field) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(text)) => Ok(Some(text)),
        _ => Err(invalid("optional payload string has an invalid shape")),
    }
}

pub(super) fn map_event(request: &Request) -> Result<Option<HookEvent>, AdapterError> {
    let native = native_event(request)?;
    let kind = match native {
        "SessionStart" => EventKind::SessionStart,
        "UserPromptSubmit" => EventKind::Prompt,
        "PostToolUse" | "PostToolUseFailure" => EventKind::ToolDone,
        "Stop" => EventKind::TurnEnd,
        "SessionEnd" => EventKind::SessionEnd,
        _ => return Ok(None),
    };
    let payload = &request.payload;
    let mut event = HookEvent::new(Host::Claude, kind, string(payload, "session_id")?);
    event.turn_id = optional_string(payload, "prompt_id")?.map(str::to_owned);
    event.cwd = optional_string(payload, "cwd")?
        .or(request.cwd.as_deref())
        .map(PathBuf::from);
    event.timestamp = optional_string(payload, "timestamp")?.and_then(super::time::parse);
    if kind == EventKind::Prompt {
        event.prompt = Some(string(payload, "prompt")?.to_owned());
    }
    if kind == EventKind::ToolDone {
        let tool = string(payload, "tool_name")?;
        let input = payload
            .get("tool_input")
            .filter(|input| input.is_object())
            .ok_or_else(|| invalid("tool_input must be an object"))?;
        let response = payload.get("tool_response").unwrap_or(&Value::Null);
        let failure = native == "PostToolUseFailure";
        let output = if failure {
            Value::String(string(payload, "error")?.to_owned())
        } else {
            response.clone()
        };
        let duration_ms = match payload.get("duration_ms") {
            None | Some(Value::Null) => None,
            Some(value) => Some(
                value
                    .as_u64()
                    .ok_or_else(|| invalid("duration_ms must be an unsigned integer"))?,
            ),
        };
        event.call = Some(Call {
            tool: tool.to_owned(),
            args_digest: digest::args_digest(tool, input),
            output_digest: digest::output_digest(&output),
            exit_code: if failure {
                parse_exit(&digest::output_text(&output))
            } else {
                response
                    .get("exit_code")
                    .and_then(Value::as_i64)
                    .and_then(|code| i32::try_from(code).ok())
            },
            is_error: failure,
            duration_ms,
            edits: if failure {
                Vec::new()
            } else {
                edits(tool, input, response, event.cwd.as_deref())
            },
        });
    }
    Ok(Some(event))
}

pub(super) fn parse_exit(text: &str) -> Option<i32> {
    text.lines()
        .next()?
        .strip_prefix("Exit code ")?
        .split_whitespace()
        .next()?
        .parse()
        .ok()
}

fn normalized(path: &Path) -> Option<PathBuf> {
    let mut result = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                if !result.pop() {
                    return None;
                }
            }
            _ => result.push(component.as_os_str()),
        }
    }
    Some(result)
}

pub(super) fn edits(
    tool: &str,
    input: &Value,
    response: &Value,
    cwd: Option<&Path>,
) -> Vec<FileEdit> {
    if !matches!(tool, "Write" | "Edit" | "MultiEdit" | "NotebookEdit") {
        return Vec::new();
    }
    let Some(cwd) = cwd.and_then(normalized) else {
        return Vec::new();
    };
    let Some(file) = input.get("file_path").and_then(Value::as_str) else {
        return Vec::new();
    };
    let file = Path::new(file);
    let file = if file.is_absolute() || file.starts_with(&cwd) {
        file.to_path_buf()
    } else {
        cwd.join(file)
    };
    let Some(file) = normalized(&file) else {
        return Vec::new();
    };
    let Ok(relative) = file.strip_prefix(cwd) else {
        return Vec::new();
    };
    if relative.as_os_str().is_empty()
        || relative
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Vec::new();
    }
    vec![FileEdit {
        path: relative.to_path_buf(),
        kind: if tool == "Write" && response.get("type").and_then(Value::as_str) == Some("create") {
            EditKind::Create
        } else {
            EditKind::Modify
        },
        via: EditVia::Tool,
    }]
}
