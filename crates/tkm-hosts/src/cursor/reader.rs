//! Cursor transcript reader for conversation `.jsonl` files.

use std::collections::BTreeMap;
use std::fs::File;
use std::io::{self, BufRead, BufReader, Read};

use serde_json::Value;
use tkm_core::model::{Call, Host, Session, StopReason, Timestamp, Turn, Usage};

use crate::{AdapterError, SessionSource, digest};

const MAX_RECORD_BYTES: usize = 32 * 1024 * 1024;
const MAX_TRANSCRIPT_BYTES: usize = 256 * 1024 * 1024;

pub(super) fn read_session(source: &SessionSource) -> Result<Session, AdapterError> {
    let file = File::open(&source.path)?;
    read_from(source, file, MAX_RECORD_BYTES, MAX_TRANSCRIPT_BYTES)
}

fn read_from(
    source: &SessionSource,
    input: impl Read,
    record_limit: usize,
    file_limit: usize,
) -> Result<Session, AdapterError> {
    let session_id = if source.session_id.is_empty() {
        source
            .path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or_default()
            .to_owned()
    } else {
        source.session_id.clone()
    };

    let mut session = Session {
        agent_home: source.agent_home.clone(),
        session_id,
        repo_key: source.repo_key.clone(),
        cwd: source.cwd.clone(),
        started: Timestamp::default(),
        ended: None,
        ..Session::new(Host::Cursor)
    };

    let cap = u64::try_from(file_limit)
        .unwrap_or(u64::MAX)
        .saturating_add(1);
    let mut reader = BufReader::new(input.take(cap));
    let mut consumed = 0_usize;
    let mut line = Vec::new();

    loop {
        let (bytes, oversized) = bounded_line(&mut reader, &mut line, record_limit)?;
        if bytes == 0 {
            break;
        }
        consumed = consumed.saturating_add(bytes);
        if consumed > file_limit {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "transcript file exceeds limit",
            )
            .into());
        }

        let trimmed = line.trim_ascii();
        if trimmed.is_empty() {
            continue;
        }

        session.total_records = session.total_records.saturating_add(1);

        if oversized {
            session.skipped_records = session.skipped_records.saturating_add(1);
            continue;
        }

        let record_val: Result<Value, _> = serde_json::from_slice(trimmed);
        let record = match record_val {
            Ok(val) if val.is_object() => val,
            _ => {
                session.skipped_records = session.skipped_records.saturating_add(1);
                continue;
            }
        };

        if !process_record(&mut session, &record) {
            session.skipped_records = session.skipped_records.saturating_add(1);
        }
    }

    session.partial = session.skipped_records.saturating_mul(5) > session.total_records;
    Ok(session)
}

fn process_record(session: &mut Session, record: &Value) -> bool {
    let Some(role) = record.get("role").and_then(Value::as_str) else {
        return false;
    };
    let Some(content) = record
        .get("message")
        .and_then(Value::as_object)
        .and_then(|m| m.get("content"))
        .and_then(Value::as_array)
    else {
        return false;
    };

    match role {
        "user" => {
            let mut text_parts = Vec::new();
            for item in content {
                let Some(item_obj) = item.as_object() else {
                    continue;
                };
                if item_obj.get("type").and_then(Value::as_str) == Some("text")
                    && let Some(text) = item_obj.get("text").and_then(Value::as_str)
                {
                    text_parts.push(text);
                }
            }
            if text_parts.is_empty() {
                return false;
            }
            let raw_prompt = text_parts.join("\n");
            let prompt = strip_user_query_tags(&raw_prompt);
            session.turns.push(Turn {
                prompt,
                calls: Vec::new(),
                stopped_by: StopReason::Unknown,
                model: None,
                usage_by_model: BTreeMap::new(),
                usage: Usage::default(),
            });
            true
        }
        "assistant" => {
            let mut recognized = false;
            for item in content {
                let Some(item_obj) = item.as_object() else {
                    continue;
                };
                match item_obj.get("type").and_then(Value::as_str) {
                    Some("tool_use") => {
                        let Some(tool_name) = item_obj.get("name").and_then(Value::as_str) else {
                            continue;
                        };
                        let input = item_obj.get("input").unwrap_or(&Value::Null);
                        let call = Call {
                            tool: tool_name.to_owned(),
                            args_digest: digest::args_digest(tool_name, input),
                            output_digest: String::new(),
                            exit_code: None,
                            is_error: false,
                            duration_ms: None,
                            edits: Vec::new(),
                        };
                        if session.turns.is_empty() {
                            session.turns.push(Turn::default());
                        }
                        if let Some(current_turn) = session.turns.last_mut() {
                            current_turn.calls.push(call);
                        }
                        recognized = true;
                    }
                    Some("text") => {
                        // Assistant text is ignored per specification
                        recognized = true;
                    }
                    _ => {}
                }
            }
            recognized
        }
        _ => false,
    }
}

fn strip_user_query_tags(prompt: &str) -> String {
    let trimmed = prompt.trim();
    if let Some(inner) = trimmed
        .strip_prefix("<user_query>")
        .and_then(|s| s.strip_suffix("</user_query>"))
    {
        inner.trim().to_owned()
    } else {
        prompt
            .replace("<user_query>", "")
            .replace("</user_query>", "")
    }
}

fn bounded_line(
    input: &mut impl BufRead,
    line: &mut Vec<u8>,
    limit: usize,
) -> io::Result<(usize, bool)> {
    line.clear();
    let mut bytes = 0_usize;
    let mut oversized = false;
    loop {
        let buffer = input.fill_buf()?;
        if buffer.is_empty() {
            return Ok((bytes, oversized));
        }
        let newline = buffer.iter().position(|byte| *byte == b'\n');
        let length = newline.map_or(buffer.len(), |index| index + 1);
        let keep = length.min(limit.saturating_sub(line.len()));
        line.extend_from_slice(&buffer[..keep]);
        oversized |= keep != length;
        bytes = bytes.saturating_add(length);
        input.consume(length);
        if newline.is_some() {
            return Ok((bytes, oversized));
        }
    }
}
