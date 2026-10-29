//! Antigravity CLI transcript log reader.

use crate::{AdapterError, SessionSource, agy::map, digest};
use serde_json::Value;
use std::collections::{BTreeMap, VecDeque};
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::{Component, Path, PathBuf};
use tkm_core::model::{Call, Host, Session, StopReason, Timestamp, Turn, Usage};

fn extract_session_id_from_path(path: &Path) -> Option<String> {
    if path.extension().is_some_and(|ext| ext == "db") {
        return path
            .file_stem()
            .and_then(|s| s.to_str())
            .map(str::to_owned);
    }
    let components: Vec<_> = path.components().collect();
    for (i, comp) in components.iter().enumerate() {
        if let Component::Normal(os_str) = comp
            && os_str.to_str() == Some("brain")
            && let Some(Component::Normal(next_str)) = components.get(i + 1)
        {
            return next_str.to_str().map(str::to_owned);
        }
    }
    path.file_name()
        .and_then(|s| s.to_str())
        .map(str::to_owned)
}

fn resolve_path(source: &SessionSource) -> (PathBuf, String) {
    let path = &source.path;
    let resolved_id = if !source.session_id.is_empty() {
        source.session_id.clone()
    } else {
        extract_session_id_from_path(path).unwrap_or_default()
    };

    if path.is_file() && path.extension().is_some_and(|ext| ext == "jsonl") {
        return (path.clone(), resolved_id);
    }

    let base_dir = source
        .agent_home
        .join("antigravity-cli")
        .join("brain")
        .join(&resolved_id)
        .join(".system_generated")
        .join("logs");

    let full = base_dir.join("transcript_full.jsonl");
    if full.exists() {
        return (full, resolved_id);
    }
    let sibling = base_dir.join("transcript.jsonl");
    if sibling.exists() {
        return (sibling, resolved_id);
    }
    (full, resolved_id)
}

fn extract_prompt(content: &str) -> String {
    let start_marker = "<USER_REQUEST>";
    let end_marker = "</USER_REQUEST>";
    if let Some(start_idx) = content.find(start_marker) {
        let rest = &content[start_idx + start_marker.len()..];
        let rest = rest
            .strip_prefix("\r\n")
            .or_else(|| rest.strip_prefix('\n'))
            .unwrap_or(rest);
        if let Some(end_idx) = rest.find(end_marker) {
            let inner = &rest[..end_idx];
            let inner = inner
                .strip_suffix("\r\n")
                .or_else(|| inner.strip_suffix('\n'))
                .unwrap_or(inner);
            return inner.to_owned();
        }
    }
    content.to_owned()
}

fn strip_leading_header_lines(content: &str) -> &str {
    let mut remaining = content;
    while let Some((line, rest)) = remaining.split_once('\n') {
        let trimmed = line.trim();
        if trimmed.starts_with("Created At:") || trimmed.starts_with("Completed At:") {
            remaining = rest;
        } else {
            break;
        }
    }
    let trimmed_remaining = remaining.trim();
    if trimmed_remaining.starts_with("Created At:")
        || trimmed_remaining.starts_with("Completed At:")
    {
        remaining = "";
    }
    remaining
}

fn parse_exit_code(content: &str) -> Option<i32> {
    for line in content.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("Command exited with code ") {
            let num_str = rest.trim_end_matches('.');
            if let Ok(code) = num_str.parse::<i32>() {
                return Some(code);
            }
        }
    }
    None
}

fn parse_duration_ms(content: &str) -> Option<u64> {
    let mut created = None;
    let mut completed = None;
    for line in content.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("Created At:") {
            created = Some(rest.trim());
        } else if let Some(rest) = trimmed.strip_prefix("Completed At:") {
            completed = Some(rest.trim());
        }
    }
    let (Some(c1), Some(c2)) = (created, completed) else {
        return None;
    };
    let (Some(t1), Some(t2)) = (
        crate::claude::time::parse(c1),
        crate::claude::time::parse(c2),
    ) else {
        return None;
    };
    let diff = t2.0.checked_sub(t1.0).unwrap_or(0);
    u64::try_from(diff).ok()
}

struct Reader {
    session: Session,
    current_turn: Option<Turn>,
    pending_calls: VecDeque<Call>,
    first_created_at: Option<Timestamp>,
}

impl Reader {
    fn new(agent_home: PathBuf, session_id: String, repo_key: tkm_core::model::RepoKey, cwd: PathBuf) -> Self {
        Self {
            session: Session {
                agent_home,
                session_id,
                repo_key,
                cwd,
                ..Session::new(Host::Agy)
            },
            current_turn: None,
            pending_calls: VecDeque::new(),
            first_created_at: None,
        }
    }

    fn record(&mut self, record: &Value) -> bool {
        if self.first_created_at.is_none()
            && let Some(ts_str) = record.get("created_at").and_then(Value::as_str)
        {
            self.first_created_at = crate::claude::time::parse(ts_str);
        }

        let Some(step_type) = record.get("type").and_then(Value::as_str) else {
            return false;
        };

        match step_type {
            "USER_INPUT" => {
                if let Some(mut prev) = self.current_turn.take() {
                    if prev.stopped_by == StopReason::Unknown {
                        prev.stopped_by = StopReason::OwnStop;
                    }
                    self.session.turns.push(prev);
                }
                let content = record
                    .get("content")
                    .and_then(Value::as_str)
                    .unwrap_or("");
                let prompt = extract_prompt(content);
                self.current_turn = Some(Turn {
                    prompt,
                    calls: Vec::new(),
                    stopped_by: StopReason::Unknown,
                    model: None,
                    usage_by_model: BTreeMap::new(),
                    usage: Usage::default(),
                });
                true
            }
            "PLANNER_RESPONSE" => {
                let input = record
                    .get("input_tokens")
                    .and_then(Value::as_u64)
                    .unwrap_or(0);
                let output = record
                    .get("output_tokens")
                    .and_then(Value::as_u64)
                    .unwrap_or(0);
                let cache_read = record
                    .get("cache_read_tokens")
                    .and_then(Value::as_u64)
                    .unwrap_or(0);
                let cache_write = record
                    .get("cache_write_tokens")
                    .and_then(Value::as_u64)
                    .unwrap_or(0);
                let usage = Usage {
                    input,
                    output,
                    cache_read,
                    cache_write,
                };
                self.session.usage = self.session.usage.add(usage);
                if let Some(turn) = self.current_turn.as_mut() {
                    turn.usage = turn.usage.add(usage);
                }
                if let Some(tool_calls) = record.get("tool_calls").and_then(Value::as_array) {
                    for tc in tool_calls {
                        let tool_name = tc.get("name").and_then(Value::as_str).unwrap_or("");
                        let args = tc.get("args").unwrap_or(&Value::Null);
                        let args_digest = map::call_args_digest(tool_name, args);
                        let edits = map::edits(tool_name, args, Some(&self.session.cwd));
                        self.pending_calls.push_back(Call {
                            tool: tool_name.to_owned(),
                            args_digest,
                            output_digest: String::new(),
                            exit_code: None,
                            is_error: false,
                            duration_ms: None,
                            edits,
                        });
                    }
                }
                true
            }
            "GENERIC" => {
                if let Some(mut call) = self.pending_calls.pop_front() {
                    let content = record
                        .get("content")
                        .and_then(Value::as_str)
                        .unwrap_or("");
                    let cleaned = strip_leading_header_lines(content);
                    call.output_digest = digest::output_digest(&Value::String(cleaned.to_owned()));
                    call.exit_code = parse_exit_code(content);
                    let status = record
                        .get("status")
                        .and_then(Value::as_str)
                        .unwrap_or("");
                    let error_val = record
                        .get("error")
                        .and_then(Value::as_str)
                        .unwrap_or("");
                    call.is_error = status == "ERROR" || !error_val.is_empty();
                    call.duration_ms = parse_duration_ms(content);
                    if let Some(turn) = self.current_turn.as_mut() {
                        turn.calls.push(call);
                    }
                }
                true
            }
            "SYSTEM_MESSAGE" | "EPHEMERAL_MESSAGE" | "CHECKPOINT" => true,
            "ERROR_MESSAGE" => {
                if let Some(turn) = self.current_turn.as_mut() {
                    turn.stopped_by = StopReason::Error;
                }
                true
            }
            _ => false,
        }
    }
}

pub(super) fn read_session(source: &SessionSource) -> Result<Session, AdapterError> {
    let (resolved_path, session_id) = resolve_path(source);
    let file = File::open(&resolved_path)?;
    let mut reader = Reader::new(
        source.agent_home.clone(),
        session_id,
        source.repo_key.clone(),
        source.cwd.clone(),
    );

    let buf = BufReader::new(file);
    for line_result in buf.lines() {
        reader.session.total_records = reader.session.total_records.saturating_add(1);
        let Ok(line) = line_result else {
            reader.session.skipped_records = reader.session.skipped_records.saturating_add(1);
            continue;
        };
        let Ok(record) = serde_json::from_str::<Value>(&line) else {
            reader.session.skipped_records = reader.session.skipped_records.saturating_add(1);
            continue;
        };
        if !reader.record(&record) {
            reader.session.skipped_records = reader.session.skipped_records.saturating_add(1);
        }
    }

    if let Some(mut turn) = reader.current_turn.take() {
        if turn.stopped_by == StopReason::Unknown {
            turn.stopped_by = StopReason::OwnStop;
        }
        reader.session.turns.push(turn);
    }

    reader.session.started = reader.first_created_at.unwrap_or_default();
    reader.session.ended = None;
    reader.session.partial = reader.session.skipped_records.saturating_mul(5) > reader.session.total_records;

    Ok(reader.session)
}
