use super::{map, time};
use crate::{AdapterError, SessionSource, digest};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::fs::File;
use std::io::{self, BufRead, BufReader, Read};
use tkm_core::model::{Call, Host, Session, StopReason, Timestamp, ToolDef, ToolKind, Turn, Usage};

const MAX_RECORD_BYTES: usize = 32 * 1024 * 1024;
const MAX_TRANSCRIPT_BYTES: usize = 256 * 1024 * 1024;

struct Pending {
    turn: usize,
    call: usize,
    input: Value,
}

struct Reader {
    session: Session,
    messages: BTreeSet<String>,
    pending: BTreeMap<String, Pending>,
    calls_seen: BTreeSet<String>,
    first: Option<Timestamp>,
    last: Option<Timestamp>,
    models: BTreeMap<usize, BTreeSet<String>>,
}

pub(super) fn read(source: &SessionSource) -> Result<Session, AdapterError> {
    read_from(
        source,
        File::open(&source.path)?,
        MAX_RECORD_BYTES,
        MAX_TRANSCRIPT_BYTES,
    )
}

fn read_from(
    source: &SessionSource,
    input: impl Read,
    record_limit: usize,
    file_limit: usize,
) -> Result<Session, AdapterError> {
    let mut reader = Reader::new(source);
    let cap = u64::try_from(file_limit)
        .unwrap_or(u64::MAX)
        .saturating_add(1);
    let mut input = BufReader::new(input.take(cap));
    let mut consumed = 0_usize;
    let mut line = Vec::new();
    loop {
        let (bytes, oversized) = bounded_line(&mut input, &mut line, record_limit)?;
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
        reader.session.total_records = reader.session.total_records.saturating_add(1);
        let valid = !oversized
            && serde_json::from_slice::<Value>(&line).is_ok_and(|record| reader.record(&record));
        if !valid {
            reader.session.skipped_records = reader.session.skipped_records.saturating_add(1);
        }
    }
    reader.session.started = reader.first.unwrap_or_default();
    reader.session.ended = reader.last;
    reader.session.partial =
        reader.session.skipped_records.saturating_mul(5) > reader.session.total_records;
    Ok(reader.session)
}

/// Consume a record in buffer-sized chunks, retaining at most `limit` bytes.
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

impl Reader {
    fn new(source: &SessionSource) -> Self {
        Self {
            session: Session {
                agent_home: source.agent_home.clone(),
                session_id: source.session_id.clone(),
                repo_key: source.repo_key.clone(),
                cwd: source.cwd.clone(),
                ..Session::new(Host::Claude)
            },
            messages: BTreeSet::new(),
            pending: BTreeMap::new(),
            calls_seen: BTreeSet::new(),
            first: None,
            last: None,
            models: BTreeMap::new(),
        }
    }

    fn record(&mut self, record: &Value) -> bool {
        let Some(kind) = record.get("type").and_then(Value::as_str) else {
            return false;
        };
        if record.get("isSidechain").and_then(Value::as_bool) == Some(true) {
            return true;
        }
        let valid = match kind {
            "user" => self.user(record),
            "assistant" => self.assistant(record),
            "attachment" => {
                self.catalog(record.get("attachment").unwrap_or(&Value::Null));
                true
            }
            "api-request-shape" => {
                self.catalog(record.get("shape").unwrap_or(&Value::Null));
                true
            }
            "system"
            | "summary"
            | "file-history-snapshot"
            | "file-history-delta"
            | "queue-operation"
            | "last-prompt"
            | "api-request"
            | "api-request-blob"
            | "api-request-start"
            | "api-request-end"
            | "atis-latch"
            | "cost-state"
            | "tools-used"
            | "mode"
            | "progress"
            | "saved_hook_context"
            | "custom-title"
            | "tag"
            | "agent-name"
            | "agent-color"
            | "pr-link"
            | "title" => true,
            _ => false,
        };
        if valid
            && let Some(timestamp) = record
                .get("timestamp")
                .and_then(Value::as_str)
                .and_then(time::parse)
        {
            self.first = Some(self.first.map_or(timestamp, |first| first.min(timestamp)));
            self.last = Some(self.last.map_or(timestamp, |last| last.max(timestamp)));
        }
        valid
    }

    fn user(&mut self, record: &Value) -> bool {
        let Some(message) = record.get("message").filter(|value| value.is_object()) else {
            return false;
        };
        let Some(content) = message
            .get("content")
            .filter(|value| value.is_string() || value.is_array())
        else {
            return false;
        };
        if let Some(blocks) = content.as_array() {
            let results: Vec<_> = blocks
                .iter()
                .filter(|block| block.get("type").and_then(Value::as_str) == Some("tool_result"))
                .collect();
            if !results.is_empty() {
                for result in results {
                    self.tool_result(record, result);
                }
                return true;
            }
        }
        if record.get("isMeta").and_then(Value::as_bool) == Some(true)
            || record.get("isCompactSummary").and_then(Value::as_bool) == Some(true)
        {
            return true;
        }
        let text = content.as_str().map_or_else(
            || {
                content
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter(|block| block.get("type").and_then(Value::as_str) == Some("text"))
                    .filter_map(|block| block.get("text").and_then(Value::as_str))
                    .collect::<Vec<_>>()
                    .join("\n")
            },
            str::to_owned,
        );
        let trimmed = text.trim();
        if trimmed.starts_with("[Request interrupted by user") || trimmed.starts_with("<interrupt")
        {
            if let Some(turn) = self.session.turns.last_mut() {
                turn.stopped_by = StopReason::Interrupted;
            }
            return true;
        }
        if trimmed.is_empty()
            || [
                "<command-",
                "<local-command",
                "<system-reminder",
                "<task-notification",
                "<bash-",
                "<teammate-message",
            ]
            .iter()
            .any(|prefix| trimmed.starts_with(prefix))
        {
            return true;
        }
        self.session.turns.push(Turn {
            prompt: text,
            ..Turn::default()
        });
        true
    }

    fn assistant(&mut self, record: &Value) -> bool {
        let Some(message) = record.get("message").filter(|value| value.is_object()) else {
            return false;
        };
        let Some(content) = message.get("content").and_then(Value::as_array) else {
            return false;
        };
        let model = message.get("model").and_then(Value::as_str);
        if let Some(usage) = message.get("usage").filter(|value| value.is_object()) {
            let Some(id) = message.get("id").and_then(Value::as_str) else {
                return false;
            };
            if self.messages.insert(id.to_owned()) {
                self.add_usage(parse_usage(usage), model);
            }
        }
        let Some(turn_index) = self.session.turns.len().checked_sub(1) else {
            return true;
        };
        if let Some(model) = model {
            let models = self.models.entry(turn_index).or_default();
            models.insert(model.to_owned());
            self.session.turns[turn_index].model = if models.len() == 1 {
                Some(model.to_owned())
            } else {
                None
            };
        }
        if self.session.turns[turn_index].stopped_by != StopReason::Interrupted {
            self.session.turns[turn_index].stopped_by =
                if message.get("stop_reason").and_then(Value::as_str) == Some("end_turn") {
                    StopReason::OwnStop
                } else {
                    StopReason::Unknown
                };
        }
        for block in content {
            if block.get("type").and_then(Value::as_str) != Some("tool_use") {
                continue;
            }
            let (Some(id), Some(name), Some(input)) = (
                block.get("id").and_then(Value::as_str),
                block.get("name").and_then(Value::as_str),
                block.get("input").filter(|input| input.is_object()),
            ) else {
                continue;
            };
            if !self.calls_seen.insert(id.to_owned()) {
                continue;
            }
            let turn = &mut self.session.turns[turn_index];
            let call_index = turn.calls.len();
            turn.calls.push(Call {
                tool: name.to_owned(),
                args_digest: digest::args_digest(name, input),
                ..Call::default()
            });
            self.pending.insert(
                id.to_owned(),
                Pending {
                    turn: turn_index,
                    call: call_index,
                    input: input.clone(),
                },
            );
        }
        true
    }

    fn tool_result(&mut self, record: &Value, result: &Value) {
        let Some(id) = result.get("tool_use_id").and_then(Value::as_str) else {
            return;
        };
        let Some(pending) = self.pending.remove(id) else {
            return;
        };
        let Some(call) = self
            .session
            .turns
            .get_mut(pending.turn)
            .and_then(|turn| turn.calls.get_mut(pending.call))
        else {
            return;
        };
        call.is_error = result
            .get("is_error")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let content = result.get("content").unwrap_or(&Value::Null);
        let output = if call.is_error {
            content
        } else {
            // A structured result that carries metadata only (an exit code) must not hide the text.
            record
                .get("toolUseResult")
                .filter(|result| carries_text(result))
                .unwrap_or(content)
        };
        call.output_digest = digest::output_digest(output);
        let text = digest::output_text(content);
        call.exit_code = map::parse_exit(&text);
        let response = record.get("toolUseResult").unwrap_or(&Value::Null);
        if call.exit_code.is_none() {
            call.exit_code = response
                .get("exit_code")
                .and_then(Value::as_i64)
                .and_then(|code| i32::try_from(code).ok());
        }
        call.duration_ms = record
            .get("duration_ms")
            .or_else(|| response.get("duration_ms"))
            .and_then(Value::as_u64);
        call.edits = if call.is_error {
            Vec::new()
        } else {
            map::edits(
                &call.tool,
                &pending.input,
                response,
                Some(&self.session.cwd),
            )
        };
    }

    fn add_usage(&mut self, usage: Usage, model: Option<&str>) {
        self.session.usage = self.session.usage + usage;
        if let Some(model) = model {
            let total = self
                .session
                .usage_by_model
                .entry(model.to_owned())
                .or_default();
            *total = *total + usage;
        }
        if let Some(turn) = self.session.turns.last_mut() {
            turn.usage = turn.usage + usage;
            if let Some(model) = model {
                let total = turn.usage_by_model.entry(model.to_owned()).or_default();
                *total = *total + usage;
            }
        }
    }

    fn add_tool(&mut self, name: &str) {
        if self
            .session
            .tool_catalog
            .iter()
            .any(|tool| tool.name == name)
        {
            return;
        }
        let server = name
            .strip_prefix("mcp__")
            .and_then(|rest| rest.split_once("__"))
            .map(|(server, _)| server.to_owned());
        self.session.tool_catalog.push(ToolDef {
            name: name.to_owned(),
            kind: if server.is_some() {
                ToolKind::Mcp
            } else {
                ToolKind::Builtin
            },
            server,
        });
    }

    fn catalog(&mut self, source: &Value) {
        if let Some(tools) = source.get("tools").and_then(Value::as_array) {
            for tool in tools {
                if let Some(name) = tool
                    .as_str()
                    .or_else(|| tool.get("name").and_then(Value::as_str))
                {
                    self.add_tool(name);
                }
            }
        }
        // The deferred-tools attachment exposes loaded tools not yet invoked.
        if source.get("type").and_then(Value::as_str) == Some("deferred_tools_delta")
            && let Some(names) = source.get("addedNames").and_then(Value::as_array)
        {
            for name in names.iter().filter_map(Value::as_str) {
                self.add_tool(name);
            }
        }
        if let Some(definitions) = source.get("surfacedDefinitions").and_then(Value::as_array) {
            for definition in definitions {
                if let Some(name) = definition.get("name").and_then(Value::as_str) {
                    self.add_tool(name);
                }
            }
        }
    }
}

fn parse_usage(value: &Value) -> Usage {
    Usage {
        input: value
            .get("input_tokens")
            .and_then(Value::as_u64)
            .unwrap_or(0),
        output: value
            .get("output_tokens")
            .and_then(Value::as_u64)
            .unwrap_or(0),
        cache_read: value
            .get("cache_read_input_tokens")
            .and_then(Value::as_u64)
            .unwrap_or(0),
        cache_write: value
            .get("cache_creation_input_tokens")
            .and_then(Value::as_u64)
            .unwrap_or(0),
    }
}

fn carries_text(result: &Value) -> bool {
    match result {
        Value::Object(object) => ["stdout", "stderr", "text", "content", "output", "file"]
            .into_iter()
            .any(|key| object.contains_key(key)),
        Value::String(_) | Value::Array(_) => true,
        _ => false,
    }
}

#[cfg(test)]
mod review_tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;
    use serde_json::json;
    use std::io::Cursor;

    fn source() -> SessionSource {
        SessionSource {
            path: "unused".into(),
            agent_home: "/synthetic".into(),
            session_id: "s".into(),
            repo_key: tkm_core::model::RepoKey("repo".into()),
            cwd: "/repo".into(),
        }
    }

    #[test]
    fn oversized_record_is_skipped_in_chunks_and_next_record_is_read() {
        let text = format!("{}\n{{\"type\":\"system\"}}\n", "x".repeat(4096));
        let mut input = std::io::BufReader::with_capacity(8, Cursor::new(text.as_bytes()));
        let mut line = Vec::new();
        let (bytes, oversized) = bounded_line(&mut input, &mut line, 32).unwrap();
        assert_eq!(bytes, 4097);
        assert!(oversized);
        assert!(line.len() <= 32);
        let session = read_from(&source(), Cursor::new(text), 32, 8192).unwrap();
        assert_eq!((session.total_records, session.skipped_records), (2, 1));
    }

    #[test]
    fn transcript_total_size_limit_is_enforced() {
        let text = "{\"type\":\"system\"}\n".repeat(10);
        assert!(matches!(
            read_from(&source(), Cursor::new(text), 32, 50),
            Err(AdapterError::Io(_))
        ));
    }

    #[test]
    fn completed_calls_release_pending_inputs_and_keep_id_deduplication() {
        let mut reader = Reader::new(&source());
        assert!(reader.record(&json!({"type":"user","message":{"content":"prompt"}})));
        let assistant = json!({"type":"assistant","message":{"content":[{"type":"tool_use","id":"c","name":"Write","input":{"file_path":"a.txt","content":"x".repeat(8192)}}]}});
        assert!(reader.record(&assistant));
        let result = json!({"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":"c","content":"done"}]}});
        assert!(reader.record(&result));
        assert_eq!(reader.pending.len(), 0);
        assert!(reader.record(&assistant));
        assert!(reader.record(&result));
        assert_eq!(reader.session.turns[0].calls.len(), 1);
        assert_eq!(reader.pending.len(), 0);
    }
}
