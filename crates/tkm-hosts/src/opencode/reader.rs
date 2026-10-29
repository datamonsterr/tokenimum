use super::map;
use crate::{AdapterError, SessionSource, digest, writer::invalid};
use rusqlite::{Connection, OpenFlags, OptionalExtension};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};
use tkm_core::model::{Call, Host, Session, StopReason, Timestamp, ToolDef, ToolKind, Turn, Usage};

const BUILTIN_TOOLS: [&str; 22] = [
    "bash",
    "read",
    "edit",
    "write",
    "glob",
    "grep",
    "list",
    "patch",
    "apply_patch",
    "multiedit",
    "webfetch",
    "websearch",
    "codesearch",
    "task",
    "todowrite",
    "todoread",
    "skill",
    "lsp",
    "question",
    "plan_enter",
    "plan_exit",
    "invalid",
];

fn sql(error: rusqlite::Error) -> AdapterError {
    AdapterError::Io(io::Error::other(error))
}

fn database(path: &Path) -> Option<PathBuf> {
    if path.is_file() {
        return Some(path.to_path_buf());
    }
    let candidate = path.join("opencode.db");
    candidate.is_file().then_some(candidate)
}

fn json_files(path: &Path) -> Vec<Value> {
    let Ok(entries) = std::fs::read_dir(path) else {
        return Vec::new();
    };
    let mut paths: Vec<_> = entries
        .flatten()
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_file()))
        .map(|entry| entry.path())
        .collect();
    paths.sort();
    paths
        .into_iter()
        .map(|path| {
            std::fs::read_to_string(path)
                .ok()
                .and_then(|text| parse(&text))
                .unwrap_or(Value::Null)
        })
        .collect()
}

fn legacy(source: &SessionSource) -> Option<Value> {
    let storage = if source
        .path
        .file_name()
        .is_some_and(|name| name == "storage")
    {
        source.path.clone()
    } else {
        source.path.join("storage")
    };
    let sessions = std::fs::read_dir(storage.join("session")).ok()?;
    let info = sessions
        .flatten()
        .filter_map(|entry| {
            std::fs::read_to_string(entry.path().join(format!("{}.json", source.session_id))).ok()
        })
        .filter_map(|text| parse(&text))
        .find(|value| {
            value.get("id").and_then(Value::as_str) == Some(source.session_id.as_str())
        })?;
    let mut messages = json_files(&storage.join("message").join(&source.session_id));
    messages.sort_by_key(|message| {
        (
            message
                .pointer("/time/created")
                .and_then(Value::as_i64)
                .unwrap_or_default(),
            message
                .get("id")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned(),
        )
    });
    let messages: Vec<_> = messages
        .into_iter()
        .map(|info| {
            let parts = info
                .get("id")
                .and_then(Value::as_str)
                .filter(|id| {
                    !id.is_empty()
                        && !id.contains('/')
                        && !id.contains('\\')
                        && *id != "."
                        && *id != ".."
                })
                .map_or_else(Vec::new, |id| json_files(&storage.join("part").join(id)));
            serde_json::json!({"info": info, "parts": parts})
        })
        .collect();
    Some(serde_json::json!({"info": info, "messages": messages}))
}

fn export(source: &SessionSource) -> Result<Value, AdapterError> {
    let mut command = Command::new("opencode");
    command
        .args(["export", &source.session_id])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    if source.cwd.is_dir() {
        command.current_dir(&source.cwd);
    }
    let mut child = command.spawn()?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| invalid("opencode export stdout unavailable"))?;
    let (sender, receiver) = std::sync::mpsc::sync_channel(1);
    std::thread::spawn(move || {
        let mut bytes = Vec::new();
        let result = stdout
            .take(256 * 1024 * 1024 + 1)
            .read_to_end(&mut bytes)
            .map(|_| bytes);
        let _ = sender.send(result);
    });
    let deadline = Instant::now() + Duration::from_secs(30);
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err(invalid("opencode export timed out"));
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    if !status.success() {
        return Err(invalid("opencode export failed"));
    }
    let bytes = receiver
        .recv_timeout(deadline.saturating_duration_since(Instant::now()))
        .map_err(|_| invalid("opencode export read timed out"))??;
    if bytes.len() > 256 * 1024 * 1024 {
        return Err(invalid("opencode export too large"));
    }
    Ok(serde_json::from_slice(&bytes)?)
}

fn read_json(source: &SessionSource, value: &Value) -> Result<Session, AdapterError> {
    let info = value
        .get("info")
        .or_else(|| value.get("session"))
        .ok_or_else(|| invalid("opencode export missing session"))?;
    if info.get("id").and_then(Value::as_str) != Some(source.session_id.as_str()) {
        return Err(invalid("opencode export session mismatch"));
    }
    let mut reader = Reader {
        session: Session {
            agent_home: source.agent_home.clone(),
            session_id: source.session_id.clone(),
            repo_key: source.repo_key.clone(),
            cwd: source.cwd.clone(),
            started: Timestamp(
                info.pointer("/time/created")
                    .and_then(Value::as_i64)
                    .unwrap_or_default(),
            ),
            ended: info
                .pointer("/time/updated")
                .and_then(Value::as_i64)
                .map(Timestamp),
            ..Session::new(Host::Opencode)
        },
        models: BTreeSet::new(),
    };
    let messages = value
        .get("messages")
        .and_then(Value::as_array)
        .ok_or_else(|| invalid("opencode export missing messages"))?;
    for entry in messages {
        reader.session.total_records = reader.session.total_records.saturating_add(1);
        let parts = entry
            .get("parts")
            .and_then(Value::as_array)
            .map_or(&[][..], Vec::as_slice);
        reader.session.total_records = reader
            .session
            .total_records
            .saturating_add(u64::try_from(parts.len()).unwrap_or(u64::MAX));
        reader.session.skipped_records = reader.session.skipped_records.saturating_add(
            u64::try_from(
                parts
                    .iter()
                    .filter(|part| !part.get("type").is_some_and(Value::is_string))
                    .count(),
            )
            .unwrap_or(u64::MAX),
        );
        if !entry
            .get("info")
            .is_some_and(|message| reader.message(message, parts))
        {
            reader.session.skipped_records = reader.session.skipped_records.saturating_add(1);
        }
    }
    reader.session.partial =
        reader.session.skipped_records.saturating_mul(5) > reader.session.total_records;
    Ok(reader.session)
}

fn parse(text: &str) -> Option<Value> {
    serde_json::from_str(text).ok().filter(Value::is_object)
}

fn tokens(message: &Value) -> Option<Usage> {
    let tokens = message.get("tokens").filter(|tokens| tokens.is_object())?;
    let count = |value: Option<&Value>| value.and_then(Value::as_u64).unwrap_or(0);
    let cache = tokens.get("cache");
    Some(Usage {
        input: count(tokens.get("input")),
        // Reasoning tokens are billed as output.
        output: count(tokens.get("output")).saturating_add(count(tokens.get("reasoning"))),
        cache_read: count(cache.and_then(|cache| cache.get("read"))),
        cache_write: count(cache.and_then(|cache| cache.get("write"))),
    })
}

struct Reader {
    session: Session,
    models: BTreeSet<String>,
}

pub(super) fn read(source: &SessionSource) -> Result<Session, AdapterError> {
    if source.session_id.is_empty()
        || source.session_id.contains('/')
        || source.session_id.contains('\\')
        || source.session_id == "."
        || source.session_id == ".."
    {
        return Err(invalid("invalid opencode session id"));
    }
    let db_error = if let Some(path) = database(&source.path) {
        match read_database(source, &path) {
            Ok(session) => return Ok(session),
            Err(error) => Some(error),
        }
    } else {
        None
    };
    if let Some(value) = legacy(source) {
        return read_json(source, &value);
    }
    match export(source).and_then(|value| read_json(source, &value)) {
        Ok(session) => Ok(session),
        Err(error) => Err(db_error.unwrap_or(error)),
    }
}

fn read_database(source: &SessionSource, path: &Path) -> Result<Session, AdapterError> {
    std::fs::metadata(path)?;
    let db = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(sql)?;
    let (created, updated) = db
        .query_row(
            "SELECT time_created, time_updated FROM session WHERE id = ?1",
            [&source.session_id],
            |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)),
        )
        .optional()
        .map_err(sql)?
        .ok_or_else(|| invalid("opencode session not found"))?;
    let mut reader = Reader {
        session: Session {
            agent_home: source.agent_home.clone(),
            session_id: source.session_id.clone(),
            repo_key: source.repo_key.clone(),
            cwd: source.cwd.clone(),
            started: Timestamp(created),
            ended: Some(Timestamp(updated)),
            ..Session::new(Host::Opencode)
        },
        models: BTreeSet::new(),
    };
    let mut grouped = reader.parts(&db, &source.session_id)?;
    let mut statement = db
        .prepare("SELECT id, data FROM message WHERE session_id = ?1 ORDER BY time_created, id")
        .map_err(sql)?;
    let rows = statement
        .query_map([&source.session_id], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .map_err(sql)?;
    for row in rows {
        let (id, data) = row.map_err(sql)?;
        reader.session.total_records = reader.session.total_records.saturating_add(1);
        let message = parse(&data);
        let parts = grouped.remove(&id).unwrap_or_default();
        if !message.is_some_and(|message| reader.message(&message, &parts)) {
            reader.session.skipped_records = reader.session.skipped_records.saturating_add(1);
        }
    }
    reader.session.partial =
        reader.session.skipped_records.saturating_mul(5) > reader.session.total_records;
    Ok(reader.session)
}

impl Reader {
    /// Loads the session's parts grouped by message, counting unreadable ones.
    fn parts(
        &mut self,
        db: &Connection,
        session_id: &str,
    ) -> Result<BTreeMap<String, Vec<Value>>, AdapterError> {
        let mut statement = db
            .prepare(
                "SELECT message_id, data FROM part WHERE session_id = ?1 ORDER BY time_created, id",
            )
            .map_err(sql)?;
        let rows = statement
            .query_map([session_id], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(sql)?;
        let mut parts: BTreeMap<String, Vec<Value>> = BTreeMap::new();
        for row in rows {
            let (message_id, data) = row.map_err(sql)?;
            self.session.total_records = self.session.total_records.saturating_add(1);
            // Any readable object with a string `type` is kept: part types the adapter
            // does not interpret (snapshot, patch, ...) are valid, not damaged.
            let known = parse(&data).filter(|part| part.get("type").is_some_and(Value::is_string));
            if let Some(part) = known {
                parts.entry(message_id).or_default().push(part);
            } else {
                self.session.skipped_records = self.session.skipped_records.saturating_add(1);
            }
        }
        Ok(parts)
    }

    fn message(&mut self, message: &Value, parts: &[Value]) -> bool {
        match message.get("role").and_then(Value::as_str) {
            Some("user") => {
                let prompt = map::prompt_text(parts);
                if !prompt.trim().is_empty() {
                    self.models.clear();
                    self.session.turns.push(Turn {
                        prompt,
                        ..Turn::default()
                    });
                }
                true
            }
            Some("assistant") => {
                self.assistant(message, parts);
                true
            }
            Some(_) => true,
            None => false,
        }
    }

    fn assistant(&mut self, message: &Value, parts: &[Value]) {
        let model = message.get("modelID").and_then(Value::as_str);
        if let Some(usage) = tokens(message) {
            self.add_usage(usage, model);
        }
        let Some(turn) = self.session.turns.last_mut() else {
            return;
        };
        if let Some(model) = model {
            self.models.insert(model.to_owned());
            turn.model = if self.models.len() == 1 {
                Some(model.to_owned())
            } else {
                None
            };
        }
        turn.stopped_by = stop_reason(message);
        let cwd = self.session.cwd.clone();
        let mut names = Vec::new();
        for part in parts
            .iter()
            .filter(|part| part.get("type").and_then(Value::as_str) == Some("tool"))
        {
            if let Some(call) = call(part, &cwd) {
                names.push(call.tool.clone());
                turn.calls.push(call);
            } else {
                self.session.skipped_records = self.session.skipped_records.saturating_add(1);
            }
        }
        for name in names {
            self.add_tool(&name);
        }
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
        // MCP tools are `<server>_<tool>` with both sides sanitized, so the
        // server cannot be recovered reliably from the name alone.
        let kind = if BUILTIN_TOOLS.contains(&name) {
            ToolKind::Builtin
        } else if name.contains('_') {
            ToolKind::Mcp
        } else {
            ToolKind::Other
        };
        self.session.tool_catalog.push(ToolDef {
            name: name.to_owned(),
            kind,
            server: None,
        });
    }
}

fn stop_reason(message: &Value) -> StopReason {
    match message.get("error") {
        Some(error) if error.get("name").and_then(Value::as_str) == Some("MessageAbortedError") => {
            StopReason::Interrupted
        }
        Some(error) if !error.is_null() => StopReason::Error,
        _ if message.get("finish").and_then(Value::as_str) == Some("stop") => StopReason::OwnStop,
        _ => StopReason::Unknown,
    }
}

fn call(part: &Value, cwd: &Path) -> Option<Call> {
    let tool = part.get("tool").and_then(Value::as_str)?;
    let state = part.get("state").filter(|state| state.is_object())?;
    let input = match state.get("input") {
        Some(input) if input.is_object() => input.clone(),
        _ => Value::Object(serde_json::Map::new()),
    };
    let metadata = state.get("metadata").unwrap_or(&Value::Null);
    let status = state.get("status").and_then(Value::as_str)?;
    let failed = status == "error";
    let output = match status {
        "completed" => state.get("output"),
        "error" => state.get("error"),
        _ => None,
    };
    let time = |key: &str| {
        state
            .pointer(&format!("/time/{key}"))
            .and_then(Value::as_u64)
    };
    Some(Call {
        tool: tool.to_owned(),
        args_digest: digest::args_digest(tool, &input),
        output_digest: output.map(digest::output_digest).unwrap_or_default(),
        exit_code: metadata
            .get("exit")
            .and_then(Value::as_i64)
            .and_then(|code| i32::try_from(code).ok()),
        is_error: failed,
        duration_ms: time("start")
            .zip(time("end"))
            .map(|(start, end)| end.saturating_sub(start)),
        edits: if status == "completed" {
            map::edits(tool, &input, metadata, Some(cwd))
        } else {
            Vec::new()
        },
    })
}
