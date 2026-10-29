use crate::{AdapterError, digest, shim, writer::invalid};
use serde_json::Value;
use std::path::{Path, PathBuf};
use tkm_core::model::{Call, EditKind, EditVia, FileEdit, HookEvent, Timestamp};
use tkm_proto::{EventKind, Host, Request};

fn string<'a>(value: &'a Value, pointer: &str) -> Result<&'a str, AdapterError> {
    value
        .pointer(pointer)
        .and_then(Value::as_str)
        .ok_or_else(|| invalid("required payload string is missing or invalid"))
}

fn millis(value: &Value, pointer: &str) -> Option<Timestamp> {
    value
        .pointer(pointer)
        .and_then(Value::as_i64)
        .map(Timestamp)
}

pub(super) fn map_event(request: &Request) -> Result<Option<HookEvent>, AdapterError> {
    let native = super::native(request)?;
    let payload = &request.payload;
    let kind = match (native.hook, native.event_type) {
        ("event", Some("session.created")) => EventKind::SessionStart,
        ("chat.message", None) => EventKind::Prompt,
        ("tool.execute.after", None) => EventKind::ToolDone,
        ("event", Some("session.idle")) => EventKind::TurnEnd,
        ("event", Some("session.deleted")) => EventKind::SessionEnd,
        _ => return Ok(None),
    };
    let session_id = match kind {
        EventKind::SessionStart | EventKind::SessionEnd => {
            string(payload, "/event/properties/info/id")?
        }
        EventKind::TurnEnd => string(payload, "/event/properties/sessionID")?,
        _ => string(payload, "/input/sessionID")?,
    };
    let mut event = HookEvent::new(Host::Opencode, kind, session_id);
    event.cwd = payload
        .pointer("/event/properties/info/directory")
        .and_then(Value::as_str)
        .or(request.cwd.as_deref())
        .map(PathBuf::from);
    match kind {
        EventKind::SessionStart => {
            event.timestamp = millis(payload, "/event/properties/info/time/created");
        }
        EventKind::Prompt => {
            event.turn_id = Some(string(payload, "/output/message/id")?.to_owned());
            event.timestamp = millis(payload, "/output/message/time/created");
            let parts = payload
                .pointer("/output/parts")
                .and_then(Value::as_array)
                .ok_or_else(|| invalid("output parts must be an array"))?;
            event.prompt = Some(prompt_text(parts));
        }
        EventKind::ToolDone => event.call = Some(call(payload, event.cwd.as_deref())?),
        _ => {}
    }
    Ok(Some(event))
}

/// Text of the person's own text parts: synthetic (injected) parts are excluded.
pub(super) fn prompt_text(parts: &[Value]) -> String {
    parts
        .iter()
        .filter(|part| part.get("type").and_then(Value::as_str) == Some("text"))
        .filter(|part| {
            !["synthetic", "ignored"]
                .iter()
                .any(|flag| part.get(*flag).and_then(Value::as_bool) == Some(true))
        })
        .filter_map(|part| part.get("text").and_then(Value::as_str))
        .collect::<Vec<_>>()
        .join("\n")
}

fn call(payload: &Value, cwd: Option<&Path>) -> Result<Call, AdapterError> {
    let tool = string(payload, "/input/tool")?;
    let args = match payload.pointer("/input/args") {
        None | Some(Value::Null) => Value::Object(serde_json::Map::new()),
        Some(args) => args.clone(),
    };
    let output = payload.pointer("/output/output").unwrap_or(&Value::Null);
    let metadata = payload.pointer("/output/metadata").unwrap_or(&Value::Null);
    Ok(Call {
        tool: tool.to_owned(),
        args_digest: digest::args_digest(tool, &args),
        output_digest: digest::output_digest(output),
        exit_code: metadata
            .get("exit")
            .and_then(Value::as_i64)
            .and_then(|code| i32::try_from(code).ok()),
        is_error: metadata
            .get("error")
            .is_some_and(|error| !error.is_null() && *error != Value::Bool(false)),
        duration_ms: None,
        edits: edits(tool, &args, metadata, cwd),
    })
}

/// File edits of opencode's path-taking edit tools, relative to the session
/// directory. `patch` and `apply_patch` carry paths inside patch text and are
/// not parsed.
pub(super) fn edits(
    tool: &str,
    args: &Value,
    metadata: &Value,
    cwd: Option<&Path>,
) -> Vec<FileEdit> {
    if !matches!(tool, "write" | "edit" | "multiedit") {
        return Vec::new();
    }
    let Some(cwd) = cwd else {
        return Vec::new();
    };
    let Some(file) = args
        .get("filePath")
        .or_else(|| args.get("path"))
        .and_then(Value::as_str)
    else {
        return Vec::new();
    };
    let Some(path) = shim::repo_relative(cwd, file) else {
        return Vec::new();
    };
    let created = tool == "write" && metadata.get("exists").and_then(Value::as_bool) == Some(false);
    vec![FileEdit {
        path,
        kind: if created {
            EditKind::Create
        } else {
            EditKind::Modify
        },
        via: EditVia::Tool,
    }]
}
