//! Codex rollout reader for `.jsonl` and zstd-compressed `.jsonl.zst` files.
//!
//! Usage comes from `event_msg` `token_count` records (`last_token_usage`);
//! the parallel `token_usage_record` records carry the same numbers and are
//! ignored so nothing is counted twice.

use std::collections::BTreeMap;
use std::fs::File;
use std::io::{self, Read};
use std::path::{Path, PathBuf};

use serde_json::{Map, Value};
use tkm_core::model::{
    Call, EditKind, EditVia, FileEdit, Session, StopReason, Timestamp, Turn, Usage,
};
use tkm_proto::Host;

use super::repo_relative;
use crate::digest::{args_digest, output_digest, truncate};
use crate::{AdapterError, SessionSource};

/// Largest rollout accepted, compressed or not, once decompressed.
const MAX_ROLLOUT_BYTES: usize = 256 * 1024 * 1024;
/// Digest length cap from ADR 0011.
const MAX_DIGEST_CHARS: usize = 512;
const UNKNOWN_MODEL: &str = "unknown";
const SHELL_FLAGS: [&str; 3] = ["-c", "-lc", "-ic"];
const IGNORED_RECORDS: [&str; 5] = [
    "response_item",
    "world_state",
    "turn_context",
    "token_usage_record",
    "compacted",
];
const IGNORED_EVENTS: [&str; 12] = [
    "thread_settings_applied",
    "user_message",
    "agent_message",
    "agent_reasoning",
    "agent_reasoning_raw_content",
    "context_compacted",
    "thread_name_updated",
    "thread_rolled_back",
    "item_started",
    "error",
    "warning",
    "stream_error",
];

/// Decompresses one zstd frame, failing when the output exceeds `limit` bytes.
///
/// # Errors
/// Returns an [`io::Error`] for invalid or truncated zstd data, or when the
/// decompressed size would exceed `limit` (decompression-bomb guard).
pub fn decompress_zstd(bytes: &[u8], limit: usize) -> io::Result<Vec<u8>> {
    let mut decoder = ruzstd::decoding::StreamingDecoder::new(bytes).map_err(io::Error::other)?;
    let allowance = u64::try_from(limit).unwrap_or(u64::MAX).saturating_add(1);
    let mut out = Vec::new();
    decoder.by_ref().take(allowance).read_to_end(&mut out)?;
    if out.len() > limit {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("decompressed rollout exceeds limit of {limit} bytes"),
        ));
    }
    Ok(out)
}

fn load(path: &Path) -> Result<Vec<u8>, AdapterError> {
    let cap = u64::try_from(MAX_ROLLOUT_BYTES).unwrap_or(u64::MAX);
    let mut raw = Vec::new();
    File::open(path)?
        .take(cap.saturating_add(1))
        .read_to_end(&mut raw)?;
    if raw.len() > MAX_ROLLOUT_BYTES {
        return Err(
            io::Error::new(io::ErrorKind::InvalidData, "rollout file exceeds limit").into(),
        );
    }
    if path.extension().is_some_and(|ext| ext == "zst") {
        Ok(decompress_zstd(&raw, MAX_ROLLOUT_BYTES)?)
    } else {
        Ok(raw)
    }
}

pub(crate) fn read_session(source: &SessionSource) -> Result<Session, AdapterError> {
    let bytes = load(&source.path)?;
    let mut reader = Reader::new(source);
    for line in bytes.split(|byte| *byte == b'\n') {
        let line = line.trim_ascii();
        if !line.is_empty() {
            reader.record(line);
        }
    }
    Ok(reader.finish())
}

struct Reader {
    session: Session,
    turns: Vec<Turn>,
    open: Option<usize>,
    model: Option<String>,
    last_seen: Option<Timestamp>,
}

impl Reader {
    fn new(source: &SessionSource) -> Self {
        let session = Session {
            agent_home: source.agent_home.clone(),
            session_id: source.session_id.clone(),
            repo_key: source.repo_key.clone(),
            cwd: source.cwd.clone(),
            ..Session::new(Host::Codex)
        };
        Self {
            session,
            turns: Vec::new(),
            open: None,
            model: None,
            last_seen: None,
        }
    }

    /// Counts one line; unusable lines are counted as skipped.
    fn record(&mut self, line: &[u8]) {
        self.session.total_records += 1;
        if !self.apply(line) {
            self.session.skipped_records += 1;
        }
    }

    fn apply(&mut self, line: &[u8]) -> bool {
        let Ok(text) = std::str::from_utf8(line) else {
            return false;
        };
        let Ok(Value::Object(record)) = serde_json::from_str::<Value>(text) else {
            return false;
        };
        let empty = Map::new();
        let payload = record
            .get("payload")
            .and_then(Value::as_object)
            .unwrap_or(&empty);
        let known = match record.get("type").and_then(Value::as_str) {
            Some("session_meta") => {
                self.session_meta(payload);
                true
            }
            Some("event_msg") => self.event(payload),
            Some("turn_context") => {
                self.turn_context(payload);
                true
            }
            Some(other) => IGNORED_RECORDS.contains(&other),
            None => false,
        };
        if known {
            let stamp = record
                .get("timestamp")
                .and_then(Value::as_str)
                .and_then(parse_timestamp);
            self.last_seen = stamp.or(self.last_seen);
        }
        known
    }

    fn session_meta(&mut self, payload: &Map<String, Value>) {
        if let Some(id) = payload.get("id").and_then(Value::as_str) {
            id.clone_into(&mut self.session.session_id);
        }
        if let Some(cwd) = payload.get("cwd").and_then(Value::as_str) {
            self.session.cwd = PathBuf::from(cwd);
        }
        if let Some(started) = payload
            .get("timestamp")
            .and_then(Value::as_str)
            .and_then(parse_timestamp)
        {
            self.session.started = started;
        }
    }

    fn turn_context(&mut self, payload: &Map<String, Value>) {
        if let Some(model) = payload.get("model").and_then(Value::as_str) {
            self.model = Some(model.to_owned());
            if let Some(index) = self.open {
                self.turns[index].model = self.model.clone();
            }
        }
    }

    /// The open turn, starting an implicit one when the log has no `task_started`.
    fn turn(&mut self) -> &mut Turn {
        let index = self.open.unwrap_or_else(|| self.start_turn());
        &mut self.turns[index]
    }

    fn start_turn(&mut self) -> usize {
        self.turns.push(Turn {
            model: self.model.clone(),
            ..Turn::default()
        });
        let index = self.turns.len() - 1;
        self.open = Some(index);
        index
    }

    fn finish_turn(&mut self, reason: StopReason) {
        if let Some(index) = self.open.take() {
            self.turns[index].stopped_by = reason;
        }
    }

    fn event(&mut self, payload: &Map<String, Value>) -> bool {
        let Some(kind) = payload.get("type").and_then(Value::as_str) else {
            return false;
        };
        match kind {
            "task_started" => {
                self.start_turn();
            }
            "task_complete" => self.finish_turn(StopReason::OwnStop),
            "turn_aborted" => self.finish_turn(StopReason::Interrupted),
            "item_completed" => {
                if let Some(item) = payload.get("item").and_then(Value::as_object) {
                    self.item(item, payload);
                }
            }
            "token_count" => self.token_count(payload),
            other => return IGNORED_EVENTS.contains(&other),
        }
        true
    }

    fn item(&mut self, item: &Map<String, Value>, event: &Map<String, Value>) {
        match item.get("type").and_then(Value::as_str) {
            Some("UserMessage") => {
                let prompt = user_text(item);
                let turn = self.turn();
                if turn.prompt.is_empty() {
                    turn.prompt = prompt;
                }
            }
            Some("CommandExecution") => {
                let call = command_call(item);
                self.turn().calls.push(call);
            }
            Some("FileChange") => {
                let call = patch_call(item, event, &self.session.cwd);
                self.turn().calls.push(call);
            }
            Some("McpToolCall") => {
                let call = mcp_call(item);
                self.turn().calls.push(call);
            }
            _ => {}
        }
    }

    fn token_count(&mut self, payload: &Map<String, Value>) {
        let Some(last) = payload
            .get("info")
            .and_then(|info| info.get("last_token_usage"))
            .and_then(Value::as_object)
        else {
            return;
        };
        let count = |key: &str| last.get(key).and_then(Value::as_u64).unwrap_or(0);
        let cached = count("cached_input_tokens");
        let usage = Usage {
            input: count("input_tokens").saturating_sub(cached),
            output: count("output_tokens"),
            cache_read: cached,
            cache_write: count("cache_write_input_tokens"),
        };
        let turn = self.turn();
        turn.usage = turn.usage + usage;
        let model = turn.model.clone().unwrap_or_else(|| UNKNOWN_MODEL.into());
        let slot = turn.usage_by_model.entry(model).or_default();
        *slot = *slot + usage;
    }

    fn finish(mut self) -> Session {
        let session = &mut self.session;
        let mut by_model: BTreeMap<String, Usage> = BTreeMap::new();
        for turn in &self.turns {
            session.usage = session.usage + turn.usage;
            for (model, usage) in &turn.usage_by_model {
                let slot = by_model.entry(model.clone()).or_default();
                *slot = *slot + *usage;
            }
        }
        session.usage_by_model = by_model;
        session.ended = self.last_seen;
        session.partial = session.skipped_records.saturating_mul(5) > session.total_records;
        session.turns = self.turns;
        self.session
    }
}

fn user_text(item: &Map<String, Value>) -> String {
    item.get("content")
        .and_then(Value::as_array)
        .map(|parts| {
            parts
                .iter()
                .filter(|part| part.get("type").and_then(Value::as_str) == Some("text"))
                .filter_map(|part| part.get("text").and_then(Value::as_str))
                .collect::<String>()
        })
        .unwrap_or_default()
}

/// Shell command of a `CommandExecution` item, without the `bash -lc` wrapper.
fn command_text(item: &Map<String, Value>) -> String {
    match item.get("command") {
        Some(Value::String(command)) => command.clone(),
        Some(Value::Array(parts)) => {
            let words: Vec<&str> = parts.iter().filter_map(Value::as_str).collect();
            match words.as_slice() {
                [.., flag, command] if SHELL_FLAGS.contains(flag) => (*command).to_owned(),
                _ => words.join(" "),
            }
        }
        _ => String::new(),
    }
}

fn failed(item: &Map<String, Value>) -> bool {
    matches!(
        item.get("status").and_then(Value::as_str),
        Some("failed" | "declined")
    )
}

fn duration_ms(item: &Map<String, Value>) -> Option<u64> {
    let duration = item.get("duration")?;
    let secs = duration.get("secs")?.as_u64()?;
    let nanos = duration.get("nanos").and_then(Value::as_u64).unwrap_or(0);
    secs.checked_mul(1_000)?.checked_add(nanos / 1_000_000)
}

fn command_call(item: &Map<String, Value>) -> Call {
    let exit_code = item
        .get("exit_code")
        .and_then(Value::as_i64)
        .and_then(|code| i32::try_from(code).ok());
    Call {
        tool: "Bash".into(),
        args_digest: truncate(&command_text(item), MAX_DIGEST_CHARS),
        output_digest: output_digest(item.get("aggregated_output").unwrap_or(&Value::Null)),
        exit_code,
        is_error: failed(item) || exit_code.is_some_and(|code| code != 0),
        duration_ms: duration_ms(item),
        edits: Vec::new(),
    }
}

fn patch_call(item: &Map<String, Value>, event: &Map<String, Value>, cwd: &Path) -> Call {
    let mut edits = Vec::new();
    let mut lines = Vec::new();
    if let Some(changes) = item.get("changes").and_then(Value::as_object) {
        for (path, change) in changes {
            let (kind, label) = match change.get("type").and_then(Value::as_str) {
                Some("add") => (EditKind::Create, "Add"),
                Some("update") => (EditKind::Modify, "Update"),
                Some("delete") => (EditKind::Delete, "Delete"),
                _ => continue,
            };
            if let Some(path) = repo_relative(path, Some(cwd)) {
                lines.push(format!("*** {label} File: {}", path.display()));
                edits.push(FileEdit {
                    path,
                    kind,
                    via: EditVia::Tool,
                });
            }
        }
    }
    let started = event.get("started_at_ms").and_then(Value::as_u64);
    let completed = event.get("completed_at_ms").and_then(Value::as_u64);
    Call {
        tool: "apply_patch".into(),
        args_digest: truncate(&lines.join("\n"), MAX_DIGEST_CHARS),
        output_digest: output_digest(item.get("stdout").unwrap_or(&Value::Null)),
        exit_code: None,
        is_error: failed(item),
        duration_ms: completed
            .zip(started)
            .and_then(|(end, start)| end.checked_sub(start)),
        edits,
    }
}

/// Best effort: field names follow the other `item_completed` items and have
/// not been observed in a real rollout yet.
fn mcp_call(item: &Map<String, Value>) -> Call {
    let name = item.get("tool").and_then(Value::as_str).unwrap_or_default();
    let tool = match item.get("server").and_then(Value::as_str) {
        Some(server) => format!("mcp__{server}__{name}"),
        None => name.to_owned(),
    };
    let result = item.get("result").unwrap_or(&Value::Null);
    let result_error = result.get("isError").and_then(Value::as_bool) == Some(true);
    let has_error = item.get("error").is_some_and(|error| !error.is_null());
    let args = args_digest(&tool, item.get("arguments").unwrap_or(&Value::Null));
    Call {
        tool,
        args_digest: args,
        output_digest: output_digest(result),
        exit_code: None,
        is_error: failed(item) || result_error || has_error,
        duration_ms: duration_ms(item),
        edits: Vec::new(),
    }
}

/// Parses `YYYY-MM-DDTHH:MM:SS[.fff](Z|±HH:MM)` into Unix milliseconds.
fn parse_timestamp(text: &str) -> Option<Timestamp> {
    let (date, rest) = text.split_once('T')?;
    let mut date_parts = date.splitn(3, '-');
    let year: i64 = date_parts.next()?.parse().ok()?;
    let month: i64 = date_parts.next()?.parse().ok()?;
    let day: i64 = date_parts.next()?.parse().ok()?;
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }

    let (clock, offset_minutes) = if let Some(clock) = rest.strip_suffix('Z') {
        (clock, 0)
    } else {
        let split = rest.rfind(['+', '-'])?;
        let (clock, zone) = rest.split_at(split);
        let (zone_hours, zone_minutes) = zone[1..].split_once(':')?;
        let total = zone_hours.parse::<i64>().ok()? * 60 + zone_minutes.parse::<i64>().ok()?;
        (clock, if zone.starts_with('-') { -total } else { total })
    };
    let mut clock_parts = clock.splitn(3, ':');
    let hour: i64 = clock_parts.next()?.parse().ok()?;
    let minute_of_hour: i64 = clock_parts.next()?.parse().ok()?;
    let seconds_text = clock_parts.next()?;
    let (seconds, fraction) = seconds_text.split_once('.').unwrap_or((seconds_text, ""));
    let seconds: i64 = seconds.parse().ok()?;
    if !fraction.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let millis: i64 = format!("{fraction:0<3}")[..3].parse().ok()?;

    let adjusted_year = if month <= 2 { year - 1 } else { year };
    let era = adjusted_year.div_euclid(400);
    let year_of_era = adjusted_year.rem_euclid(400);
    let month_index = (month + 9) % 12;
    let day_of_year = (153 * month_index + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    let days = era * 146_097 + day_of_era - 719_468;

    let total_minutes = (days * 24 + hour) * 60 + minute_of_hour - offset_minutes;
    Some(Timestamp((total_minutes * 60 + seconds) * 1_000 + millis))
}

#[cfg(test)]
mod timestamp_review_tests {
    #[test]
    fn timestamps_reject_overflow_and_out_of_range_fields() {
        for text in [
            "2026-02-29T00:00:00Z",
            "2026-04-31T00:00:00Z",
            "2026-10-29T24:00:00Z",
            "2026-10-29T00:60:00Z",
            "2026-10-29T00:00:60Z",
            "2026-10-29T00:00:00+24:00",
            "2026-10-29T00:00:00+01:60",
            "2026-10-29T00:00:00+9223372036854775807:00",
            "9223372036854775807-10-29T00:00:00Z",
            "2026-10-29T00:00:00.Z",
        ] {
            assert_eq!(super::parse_timestamp(text), None, "{text}");
        }
    }

    #[test]
    fn timestamps_accept_leap_days_offsets_and_fractional_boundaries() {
        use tkm_core::model::Timestamp;
        assert_eq!(
            super::parse_timestamp("1970-01-01T01:00:00.125+01:00"),
            Some(Timestamp(125))
        );
        assert_eq!(
            super::parse_timestamp("1969-12-31T23:59:59.9999Z"),
            Some(Timestamp(-1))
        );
        assert_eq!(
            super::parse_timestamp("2000-02-29T00:00:00Z"),
            Some(Timestamp(951_782_400_000))
        );
        assert!(super::parse_timestamp("0000-01-01T00:00:00Z").is_some());
        assert!(super::parse_timestamp("9999-12-31T23:59:59.999Z").is_some());
    }
}
