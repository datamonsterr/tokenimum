//! Session reader for Kimi Code logs (`context.jsonl` and `wire.jsonl`).

use std::collections::{BTreeMap, HashMap};
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

use serde_json::Value;
use tkm_core::model::{Call, Host, Session, StopReason, Timestamp, Turn, Usage};

use crate::digest::{args_digest, output_digest, truncate};
use crate::{AdapterError, SessionSource};

/// 2^63 is exactly representable as f64; values outside +/-this are out of i64 range.
const LIMIT: f64 = 9_223_372_036_854_775_808.0;

fn float_seconds_to_timestamp(seconds: f64) -> Option<Timestamp> {
    if !seconds.is_finite() {
        return None;
    }
    let millis = (seconds * 1000.0).round();
    if !(-LIMIT..LIMIT).contains(&millis) {
        return None;
    }
    #[allow(clippy::cast_possible_truncation)] // range checked above
    Some(Timestamp(millis as i64))
}

fn extract_text(content: Option<&Value>) -> String {
    let Some(content) = content else {
        return String::new();
    };
    match content {
        Value::String(s) => s.clone(),
        Value::Array(arr) => {
            let parts: Vec<&str> = arr
                .iter()
                .filter_map(|v| v.as_str().or_else(|| v.get("text").and_then(Value::as_str)))
                .collect();
            parts.join("")
        }
        _ => String::new(),
    }
}

pub(super) fn read_session(source: &SessionSource) -> Result<Session, AdapterError> {
    let session_dir = if source.path.as_os_str().is_empty() {
        let cwd_str = source.cwd.to_str().unwrap_or("");
        super::session_dir(&source.agent_home, cwd_str, &source.session_id)
    } else if source.path.is_file() {
        source.path.parent().unwrap_or(&source.path).to_path_buf()
    } else {
        source.path.clone()
    };

    let mut session = Session {
        agent_home: source.agent_home.clone(),
        session_id: source.session_id.clone(),
        repo_key: source.repo_key.clone(),
        cwd: source.cwd.clone(),
        ..Session::new(Host::Kimi)
    };

    let context_path = session_dir.join("context.jsonl");
    let mut pending_calls: HashMap<String, (usize, usize)> = HashMap::new();

    read_context(&context_path, &mut session, &mut pending_calls)?;

    let wire_path = session_dir.join("wire.jsonl");
    if wire_path.exists() {
        read_wire(&wire_path, &mut session)?;
    }

    session.partial = session.skipped_records.saturating_mul(5) > session.total_records;

    Ok(session)
}

#[expect(clippy::too_many_lines, reason = "single pass over the context log")]
fn read_context(
    path: &Path,
    session: &mut Session,
    pending_calls: &mut HashMap<String, (usize, usize)>,
) -> Result<(), AdapterError> {
    let file = File::open(path)?;
    let reader = BufReader::new(file);

    for line in reader.lines() {
        let line = line?;
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        session.total_records = session.total_records.saturating_add(1);

        let Ok(record) = serde_json::from_str::<Value>(trimmed) else {
            session.skipped_records = session.skipped_records.saturating_add(1);
            continue;
        };

        let Some(obj) = record.as_object() else {
            session.skipped_records = session.skipped_records.saturating_add(1);
            continue;
        };

        if obj.contains_key("_system_prompt")
            || obj.contains_key("_checkpoint")
            || obj.contains_key("_usage")
        {
            continue;
        }

        let Some(role) = obj.get("role").and_then(Value::as_str) else {
            session.skipped_records = session.skipped_records.saturating_add(1);
            continue;
        };

        match role {
            "user" => {
                let text = extract_text(obj.get("content"));
                if text.starts_with("<system>CHECKPOINT") {
                    continue;
                }
                session.turns.push(Turn {
                    prompt: text,
                    calls: Vec::new(),
                    stopped_by: StopReason::Unknown,
                    model: None,
                    usage_by_model: BTreeMap::new(),
                    usage: Usage::default(),
                });
            }
            "assistant" => {
                if let Some(tool_calls) = obj.get("tool_calls").and_then(Value::as_array) {
                    if session.turns.is_empty() {
                        session.turns.push(Turn::default());
                    }
                    let turn_idx = session.turns.len().saturating_sub(1);
                    for tc in tool_calls {
                        let id = tc.get("id").and_then(Value::as_str).unwrap_or("");
                        let function = tc.get("function").unwrap_or(&Value::Null);
                        let name = function.get("name").and_then(Value::as_str).unwrap_or("");
                        let args_raw = function
                            .get("arguments")
                            .and_then(Value::as_str)
                            .unwrap_or("");
                        let args = match serde_json::from_str::<Value>(args_raw) {
                            Ok(parsed) => args_digest(name, &parsed),
                            Err(_) => truncate(args_raw, 512),
                        };

                        let call = Call {
                            tool: name.to_owned(),
                            args_digest: args,
                            output_digest: String::new(),
                            exit_code: None,
                            is_error: false,
                            duration_ms: None,
                            edits: Vec::new(),
                        };

                        if let Some(turn) = session.turns.get_mut(turn_idx) {
                            let call_idx = turn.calls.len();
                            turn.calls.push(call);
                            if !id.is_empty() {
                                pending_calls.insert(id.to_owned(), (turn_idx, call_idx));
                            }
                        }
                    }
                }
            }
            "tool" => {
                let tool_call_id = obj
                    .get("tool_call_id")
                    .and_then(Value::as_str)
                    .unwrap_or("");
                let text = extract_text(obj.get("content"));
                let is_error = text.contains("<system>ERROR:");
                let out_digest = output_digest(&Value::String(text));

                if let Some(&(t_idx, c_idx)) = pending_calls.get(tool_call_id)
                    && let Some(turn) = session.turns.get_mut(t_idx)
                    && let Some(call) = turn.calls.get_mut(c_idx)
                {
                    call.output_digest = out_digest;
                    if is_error {
                        call.is_error = true;
                    }
                }
            }
            "system" => {
                // Known role, ignore.
            }
            _ => {
                session.skipped_records = session.skipped_records.saturating_add(1);
            }
        }
    }

    Ok(())
}

fn read_wire(path: &Path, session: &mut Session) -> Result<(), AdapterError> {
    let file = File::open(path)?;
    let reader = BufReader::new(file);

    let mut is_first = true;
    let mut turn_begins_count: usize = 0;
    let mut last_timestamp: Option<Timestamp> = None;
    let mut last_msg_type: Option<String> = None;

    for line in reader.lines() {
        let line = line?;
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        session.total_records = session.total_records.saturating_add(1);

        if is_first {
            is_first = false;
            continue;
        }

        let Ok(record) = serde_json::from_str::<Value>(trimmed) else {
            session.skipped_records = session.skipped_records.saturating_add(1);
            continue;
        };

        let Some(obj) = record.as_object() else {
            session.skipped_records = session.skipped_records.saturating_add(1);
            continue;
        };

        let ts = obj
            .get("timestamp")
            .and_then(Value::as_f64)
            .and_then(float_seconds_to_timestamp);

        let msg_type = obj
            .get("message")
            .and_then(|m| m.get("type"))
            .and_then(Value::as_str);

        if ts.is_none() && msg_type.is_none() {
            session.skipped_records = session.skipped_records.saturating_add(1);
            continue;
        }

        if let Some(ts_val) = ts {
            last_timestamp = Some(ts_val);
        }
        if let Some(mt) = msg_type {
            last_msg_type = Some(mt.to_owned());
        }

        match msg_type {
            Some("TurnBegin") => {
                if turn_begins_count == 0 {
                    session.started = ts.unwrap_or_default();
                }
                turn_begins_count = turn_begins_count.saturating_add(1);
            }
            Some("StatusUpdate") => {
                if let Some(usage_val) = obj
                    .get("message")
                    .and_then(|m| m.get("payload"))
                    .and_then(|p| p.get("token_usage"))
                {
                    let input = usage_val
                        .get("input_other")
                        .and_then(Value::as_u64)
                        .unwrap_or(0);
                    let output = usage_val.get("output").and_then(Value::as_u64).unwrap_or(0);
                    let cache_read = usage_val
                        .get("input_cache_read")
                        .and_then(Value::as_u64)
                        .unwrap_or(0);
                    let cache_write = usage_val
                        .get("input_cache_creation")
                        .and_then(Value::as_u64)
                        .unwrap_or(0);

                    let step_usage = Usage {
                        input,
                        output,
                        cache_read,
                        cache_write,
                    };
                    session.usage = session.usage.add(step_usage);

                    let cur_idx = turn_begins_count.saturating_sub(1);
                    if let Some(turn) = session.turns.get_mut(cur_idx) {
                        turn.usage = turn.usage.add(step_usage);
                    }
                }
            }
            Some(_) => {
                // TurnEnd is recorded in last_msg_type; other known wire event
                // variants (StepBegin, StepRetry, ...) carry nothing we use.
            }
            None => {
                session.skipped_records = session.skipped_records.saturating_add(1);
            }
        }
    }

    if last_msg_type.as_deref() == Some("TurnEnd") {
        session.ended = last_timestamp;
    } else {
        session.ended = None;
    }

    Ok(())
}
