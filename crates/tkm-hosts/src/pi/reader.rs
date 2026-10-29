use super::map;
use crate::claude::time;
use crate::{AdapterError, SessionSource, digest, writer::invalid};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::fs::File;
use std::io::{self, BufRead, BufReader, Read};
use tkm_core::model::{Call, Host, Session, StopReason, Timestamp, ToolDef, ToolKind, Turn, Usage};

const MAX_RECORD_BYTES: usize = 32 * 1024 * 1024;
const MAX_FILE_BYTES: usize = 256 * 1024 * 1024;
const BUILTIN_TOOLS: [&str; 8] = [
    "read",
    "bash",
    "powershell",
    "edit",
    "write",
    "grep",
    "find",
    "ls",
];
const KNOWN_ENTRIES: [&str; 11] = [
    "message",
    "model_change",
    "thinking_level_change",
    "usage",
    "compaction",
    "context_edit",
    "branch_summary",
    "custom",
    "custom_message",
    "label",
    "session_info",
];

struct Pending {
    turn: usize,
    call: usize,
    input: Value,
}

struct Reader {
    session: Session,
    pending: BTreeMap<String, Pending>,
    models: BTreeSet<String>,
    /// Model of the latest assistant message, for nested tool-result usage.
    last_model: Option<String>,
}

/// Entries that carry an `id`, in file order, with the tree links needed to
/// find the active branch.
#[derive(Default)]
struct Scanned {
    entries: Vec<Value>,
    index: BTreeMap<String, usize>,
    /// Last known conversation entry (not a standalone `usage` entry).
    leaf: Option<usize>,
    /// Standalone `usage` entries: spend that belongs to the session wherever they sit.
    usages: Vec<Value>,
    first: Option<Timestamp>,
    last: Option<Timestamp>,
}

impl Scanned {
    fn stamp(&mut self, record: &Value) {
        if let Some(stamp) = timestamp(record) {
            self.first = Some(self.first.map_or(stamp, |first| first.min(stamp)));
            self.last = Some(self.last.map_or(stamp, |last| last.max(stamp)));
        }
    }

    /// Positions from the root to the last entry, following `parentId`.
    fn active_path(&self) -> Vec<usize> {
        let mut path = Vec::new();
        let mut seen = BTreeSet::new();
        let mut cursor = self.leaf;
        while let Some(position) = cursor {
            if !seen.insert(position) {
                break;
            }
            path.push(position);
            cursor = self.entries[position]
                .get("parentId")
                .and_then(Value::as_str)
                .and_then(|parent| self.index.get(parent).copied());
        }
        path.reverse();
        path
    }
}

pub(super) fn read(source: &SessionSource) -> Result<Session, AdapterError> {
    let mut reader = Reader {
        session: Session {
            agent_home: source.agent_home.clone(),
            session_id: source.session_id.clone(),
            repo_key: source.repo_key.clone(),
            cwd: source.cwd.clone(),
            ..Session::new(Host::Pi)
        },
        pending: BTreeMap::new(),
        models: BTreeSet::new(),
        last_model: None,
    };
    let scanned = reader.scan(File::open(&source.path)?)?;
    reader.session.started = scanned.first.unwrap_or_default();
    reader.session.ended = scanned.last;
    // Follow the active branch: the path from the last entry back to the root.
    for position in scanned.active_path() {
        reader.entry(&scanned.entries[position]);
    }
    for entry in &scanned.usages {
        reader.usage_entry(entry);
    }
    reader.session.partial =
        reader.session.skipped_records.saturating_mul(5) > reader.session.total_records;
    Ok(reader.session)
}

impl Reader {
    /// Reads every record, counting skipped ones, and keeps the tree entries.
    fn scan(&mut self, input: impl Read) -> Result<Scanned, AdapterError> {
        let cap = u64::try_from(MAX_FILE_BYTES)
            .unwrap_or(u64::MAX)
            .saturating_add(1);
        let mut input = BufReader::new(input.take(cap));
        let mut scanned = Scanned::default();
        let mut consumed = 0_usize;
        let mut line = Vec::new();
        let mut header = false;
        loop {
            let (bytes, oversized) = bounded_line(&mut input, &mut line, MAX_RECORD_BYTES)?;
            if bytes == 0 {
                break;
            }
            consumed = consumed.saturating_add(bytes);
            if consumed > MAX_FILE_BYTES {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "session file exceeds limit",
                )
                .into());
            }
            if line.iter().all(u8::is_ascii_whitespace) {
                continue;
            }
            let record = if oversized {
                None
            } else {
                serde_json::from_slice::<Value>(&line).ok()
            };
            self.session.total_records = self.session.total_records.saturating_add(1);
            if !header {
                check_header(record.as_ref())?;
                header = true;
                scanned.stamp(record.as_ref().unwrap_or(&Value::Null));
                continue;
            }
            let Some(record) = record.filter(Value::is_object) else {
                self.session.skipped_records = self.session.skipped_records.saturating_add(1);
                continue;
            };
            let kind = record.get("type").and_then(Value::as_str);
            let known = kind.is_some_and(|kind| KNOWN_ENTRIES.contains(&kind));
            if !known {
                self.session.skipped_records = self.session.skipped_records.saturating_add(1);
            }
            scanned.stamp(&record);
            // Every record with an id joins the tree so parent links can pass through
            // entry types this reader does not interpret.
            if let Some(id) = record.get("id").and_then(Value::as_str) {
                let position = scanned.entries.len();
                scanned.index.insert(id.to_owned(), position);
                if known && kind != Some("usage") {
                    scanned.leaf = Some(position);
                }
                if kind == Some("usage") {
                    scanned.usages.push(record.clone());
                }
                scanned.entries.push(record);
            }
        }
        if !header {
            return Err(invalid("Pi session header is missing"));
        }
        Ok(scanned)
    }
}

/// The first record must be a version 2 or 3 session header.
fn check_header(record: Option<&Value>) -> Result<(), AdapterError> {
    let version = record
        .filter(|record| record.get("type").and_then(Value::as_str) == Some("session"))
        .map(|record| record.get("version").and_then(Value::as_u64).unwrap_or(1))
        .ok_or_else(|| invalid("Pi session header is missing"))?;
    if version < 2 {
        return Err(AdapterError::Unsupported {
            operation: format!("Pi session version {version}"),
        });
    }
    Ok(())
}

fn timestamp(record: &Value) -> Option<Timestamp> {
    record
        .get("timestamp")
        .and_then(Value::as_str)
        .and_then(time::parse)
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

fn count(value: &Value, key: &str) -> u64 {
    value.get(key).and_then(Value::as_u64).unwrap_or(0)
}

/// Pi reports `reasoning` inside `output`, so it is not added again.
fn usage(value: &Value) -> Usage {
    Usage {
        input: count(value, "input"),
        output: count(value, "output"),
        cache_read: count(value, "cacheRead"),
        cache_write: count(value, "cacheWrite"),
    }
}

fn text(content: &Value) -> String {
    content.as_str().map_or_else(
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
    )
}

impl Reader {
    fn entry(&mut self, entry: &Value) {
        match entry.get("type").and_then(Value::as_str) {
            Some("message") => {
                if let Some(message) = entry.get("message").filter(|value| value.is_object()) {
                    self.message(message);
                }
            }
            Some("compaction" | "branch_summary") => {
                if let Some(value) = entry.get("usage") {
                    self.add_usage(usage(value), None, false);
                }
            }
            _ => {}
        }
    }

    /// Standalone usage (cache warming and similar) counts for the session on any branch.
    fn usage_entry(&mut self, entry: &Value) {
        if let Some(value) = entry.get("usage") {
            let model = entry.get("model").and_then(Value::as_str);
            self.add_usage(usage(value), model, false);
        }
    }

    fn message(&mut self, message: &Value) {
        let content = message.get("content").unwrap_or(&Value::Null);
        match message.get("role").and_then(Value::as_str) {
            Some("user") => {
                let prompt = text(content);
                if !prompt.trim().is_empty() {
                    self.models.clear();
                    self.session.turns.push(Turn {
                        prompt,
                        ..Turn::default()
                    });
                }
            }
            Some("assistant") => self.assistant(message, content),
            Some("toolResult") => self.tool_result(message, content),
            Some("system") => {
                for tool in message
                    .get("toolsAdded")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                {
                    if let Some(name) = tool.get("name").and_then(Value::as_str) {
                        self.add_tool(name);
                    }
                }
            }
            _ => {}
        }
    }

    fn assistant(&mut self, message: &Value, content: &Value) {
        let model = message.get("model").and_then(Value::as_str);
        if let Some(model) = model {
            self.last_model = Some(model.to_owned());
        }
        if let Some(value) = message.get("usage").filter(|value| value.is_object()) {
            self.add_usage(usage(value), model, true);
        }
        let Some(turn_index) = self.session.turns.len().checked_sub(1) else {
            return;
        };
        let turn = &mut self.session.turns[turn_index];
        if let Some(model) = model {
            self.models.insert(model.to_owned());
            turn.model = if self.models.len() == 1 {
                Some(model.to_owned())
            } else {
                None
            };
        }
        turn.stopped_by = match message.get("stopReason").and_then(Value::as_str) {
            Some("stop") => StopReason::OwnStop,
            Some("aborted") => StopReason::Interrupted,
            Some("error") => StopReason::Error,
            _ => StopReason::Unknown,
        };
        for block in content.as_array().into_iter().flatten() {
            if block.get("type").and_then(Value::as_str) != Some("toolCall") {
                continue;
            }
            let (Some(id), Some(name)) = (
                block.get("id").and_then(Value::as_str),
                block.get("name").and_then(Value::as_str),
            ) else {
                continue;
            };
            let input = block
                .get("arguments")
                .filter(|input| input.is_object())
                .cloned()
                .unwrap_or_else(|| Value::Object(serde_json::Map::new()));
            let call = turn.calls.len();
            turn.calls.push(Call {
                tool: name.to_owned(),
                args_digest: digest::args_digest(name, &input),
                ..Call::default()
            });
            self.pending.insert(
                id.to_owned(),
                Pending {
                    turn: turn_index,
                    call,
                    input,
                },
            );
        }
    }

    fn tool_result(&mut self, message: &Value, content: &Value) {
        if let Some(value) = message.get("usage").filter(|value| value.is_object()) {
            // Nested model work is billed to the model that ran the turn.
            let model = self.last_model.clone();
            self.add_usage(usage(value), model.as_deref(), true);
        }
        let Some(pending) = message
            .get("toolCallId")
            .and_then(Value::as_str)
            .and_then(|id| self.pending.remove(id))
        else {
            return;
        };
        let cwd = self.session.cwd.clone();
        let Some(call) = self
            .session
            .turns
            .get_mut(pending.turn)
            .and_then(|turn| turn.calls.get_mut(pending.call))
        else {
            return;
        };
        call.is_error = message.get("isError").and_then(Value::as_bool) == Some(true);
        call.output_digest = digest::output_digest(content);
        call.edits = if call.is_error {
            Vec::new()
        } else {
            map::edits(&call.tool, &pending.input, Some(&cwd))
        };
    }

    /// Adds usage to the session; `to_turn` also adds it to the current turn.
    fn add_usage(&mut self, usage: Usage, model: Option<&str>, to_turn: bool) {
        self.session.usage = self.session.usage + usage;
        if let Some(model) = model {
            let total = self
                .session
                .usage_by_model
                .entry(model.to_owned())
                .or_default();
            *total = *total + usage;
        }
        if let (true, Some(turn)) = (to_turn, self.session.turns.last_mut()) {
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
        self.session.tool_catalog.push(ToolDef {
            name: name.to_owned(),
            kind: if BUILTIN_TOOLS.contains(&name) {
                ToolKind::Builtin
            } else {
                ToolKind::Other
            },
            server: None,
        });
    }
}
