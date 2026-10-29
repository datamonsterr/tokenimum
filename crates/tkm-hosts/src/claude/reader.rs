use super::{map, time};
use crate::{AdapterError, SessionSource, digest};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::fs::File;
use std::io::{BufRead, BufReader};
use tkm_core::model::{Call, Host, Session, StopReason, Timestamp, ToolDef, ToolKind, Turn, Usage};

struct Pending {
    turn: usize,
    call: usize,
    input: Value,
}

struct Reader {
    session: Session,
    messages: BTreeSet<String>,
    pending: BTreeMap<String, Pending>,
    first: Option<Timestamp>,
    last: Option<Timestamp>,
    models: BTreeMap<usize, BTreeSet<String>>,
}

pub(super) fn read(source: &SessionSource) -> Result<Session, AdapterError> {
    let mut reader = Reader {
        session: Session {
            agent_home: source.agent_home.clone(),
            session_id: source.session_id.clone(),
            repo_key: source.repo_key.clone(),
            cwd: source.cwd.clone(),
            ..Session::new(Host::Claude)
        },
        messages: BTreeSet::new(),
        pending: BTreeMap::new(),
        first: None,
        last: None,
        models: BTreeMap::new(),
    };
    let mut input = BufReader::new(File::open(&source.path)?);
    let mut line = Vec::new();
    loop {
        line.clear();
        if input.read_until(b'\n', &mut line)? == 0 {
            break;
        }
        reader.session.total_records = reader.session.total_records.saturating_add(1);
        let valid = serde_json::from_slice::<Value>(&line)
            .ok()
            .is_some_and(|record| reader.record(&record));
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

impl Reader {
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
            if self.pending.contains_key(id) {
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
        let Some(pending) = self.pending.get(id) else {
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
        call.output_digest = digest::output_digest(content);
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
