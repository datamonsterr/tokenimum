use super::{BenchError, Prompt, SessionMetrics, SessionSpec, Tokens, types::invalid};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::fs::File;
use std::io::{BufRead, BufReader, Read};
use tkm_hosts::{HostAdapter, SessionSource, claude::ClaudeAdapter};

/// Parsed private records and public-safe metrics. Deliberately not serializable.
#[derive(Debug)]
pub struct Transcript {
    /// Manifest metadata.
    pub spec: SessionSpec,
    /// Raw private records, never written to a report.
    pub records: Vec<Value>,
    /// Parses completely and ends on a completed assistant turn or final result.
    pub intact: bool,
    /// First recorded timestamp in Unix milliseconds.
    pub started_ms: i64,
    /// Last timestamp in Unix milliseconds.
    pub ended_ms: i64,
    /// Deduplicated assistant usage and normalized tool metrics.
    pub metrics: SessionMetrics,
}

/// Load a transcript strictly, reusing the existing Claude adapter for timing and tool metrics.
///
/// # Errors
/// Rejects malformed JSON, missing usage identity, oversize input, and unreadable files.
pub fn load_transcript(spec: &SessionSpec) -> Result<Transcript, BenchError> {
    const MAX_FILE: u64 = 256 * 1024 * 1024;
    const MAX_LINE: u64 = 32 * 1024 * 1024;
    let file = File::open(&spec.path)?;
    if file.metadata()?.len() > MAX_FILE {
        return Err(invalid("transcript exceeds size limit"));
    }
    let mut reader = BufReader::new(file);
    let mut records = Vec::new();
    loop {
        let mut line = String::new();
        let read = reader.by_ref().take(MAX_LINE + 1).read_line(&mut line)?;
        if read == 0 {
            break;
        }
        if read as u64 > MAX_LINE {
            return Err(invalid("transcript record exceeds size limit"));
        }
        if line.trim().is_empty() {
            continue;
        }
        let value: Value = serde_json::from_str(&line)?;
        if !value.is_object() || value.get("type").and_then(Value::as_str).is_none() {
            return Err(invalid("invalid transcript record"));
        }
        records.push(value);
    }
    let session = ClaudeAdapter::new().read_session(&SessionSource {
        path: spec.path.clone(),
        session_id: spec.id.clone(),
        ..SessionSource::default()
    })?;
    let last = records
        .iter()
        .rev()
        .find(|r| matches!(r["type"].as_str(), Some("user" | "assistant" | "result")));
    let intact = last.is_some_and(|r| match r["type"].as_str() {
        Some("result") => true,
        Some("assistant") => matches!(
            r["message"]["stop_reason"].as_str(),
            Some("end_turn" | "stop_sequence")
        ),
        _ => false,
    });
    let mut metrics = SessionMetrics {
        turns: extract_prompts(&records).len() as u64,
        tool_calls: session.turns.iter().map(|t| t.calls.len() as u64).sum(),
        tool_errors: session
            .turns
            .iter()
            .flat_map(|t| &t.calls)
            .filter(|c| c.is_error)
            .count() as u64,
        wall_ms: session
            .ended
            .map_or(0, |end| end.0.abs_diff(session.started.0)),
        ..SessionMetrics::default()
    };
    let (started_ms, ended_ms) = record_bounds(&records)?;
    let (tool_calls, tool_errors) = raw_tool_metrics(&records);
    metrics.tool_calls = metrics.tool_calls.max(tool_calls);
    metrics.tool_errors = metrics.tool_errors.max(tool_errors);
    metrics.wall_ms = ended_ms.abs_diff(started_ms);
    add_usage(&records, intact, &mut metrics)?;
    Ok(Transcript {
        spec: spec.clone(),
        records,
        intact,
        started_ms,
        ended_ms,
        metrics,
    })
}

fn add_usage(
    records: &[Value],
    intact: bool,
    metrics: &mut SessionMetrics,
) -> Result<(), BenchError> {
    let mut messages: BTreeMap<String, (String, Tokens)> = BTreeMap::new();
    for record in records {
        if record["type"] != "assistant" {
            continue;
        }
        let message = &record["message"];
        let Some(usage) = message.get("usage").filter(|v| v.is_object()) else {
            continue;
        };
        let id = message["id"]
            .as_str()
            .ok_or_else(|| invalid("assistant usage lacks message id"))?;
        let model = message["model"]
            .as_str()
            .ok_or_else(|| invalid("assistant usage lacks model"))?;
        let tokens = Tokens {
            input: token(usage, "input_tokens")?,
            cache_creation: token(usage, "cache_creation_input_tokens")?,
            cache_read: token(usage, "cache_read_input_tokens")?,
            output: token(usage, "output_tokens")?,
        };
        let entry = messages
            .entry(id.to_owned())
            .or_insert_with(|| (model.to_owned(), Tokens::default()));
        if entry.0 != model {
            return Err(invalid("message model changed across snapshots"));
        }
        entry.1 = entry.1.max(tokens);
    }
    if intact && messages.is_empty() {
        return Err(invalid("intact transcript has no assistant usage"));
    }
    for (model, tokens) in messages.into_values() {
        metrics.tokens.add(tokens);
        metrics.by_model.entry(model).or_default().add(tokens);
    }
    Ok(())
}

fn record_bounds(records: &[Value]) -> Result<(i64, i64), BenchError> {
    let mut bounds: Option<(i64, i64)> = None;
    for record in records {
        if let Some(value) = record.get("timestamp") {
            let timestamp = value
                .as_str()
                .and_then(tkm_hosts::claude::parse_timestamp)
                .ok_or_else(|| invalid("invalid transcript timestamp"))?
                .0;
            bounds = Some(bounds.map_or((timestamp, timestamp), |(first, last)| {
                (first.min(timestamp), last.max(timestamp))
            }));
        }
    }
    bounds.ok_or_else(|| invalid("transcript has no timestamp"))
}

fn raw_tool_metrics(records: &[Value]) -> (u64, u64) {
    let mut calls = BTreeSet::new();
    let mut errors = BTreeSet::new();
    for record in records {
        if let Some(blocks) = record["message"]["content"].as_array() {
            for block in blocks {
                if record["type"] == "assistant"
                    && block["type"] == "tool_use"
                    && let Some(id) = block["id"].as_str()
                {
                    calls.insert(id);
                }
                if record["type"] == "user"
                    && block["type"] == "tool_result"
                    && block["is_error"] == true
                    && let Some(id) = block["tool_use_id"].as_str()
                {
                    errors.insert(id);
                }
            }
        }
    }
    (
        calls.len() as u64,
        calls.intersection(&errors).count() as u64,
    )
}

fn token(usage: &Value, key: &str) -> Result<u64, BenchError> {
    usage.get(key).map_or(Ok(0), |v| {
        v.as_u64().ok_or_else(|| invalid("invalid token count"))
    })
}

fn injected(text: &str) -> bool {
    let text = text.trim();
    text.is_empty()
        || [
            "<task-notification",
            "<system-reminder",
            "<hook-feedback",
            "<hook_feedback",
            "Hook feedback:",
            "Stop hook feedback:",
            "[Request interrupted by user",
            "<interrupt",
            "<command-",
            "<local-command",
            "<bash-",
            "<teammate-message",
        ]
        .iter()
        .any(|prefix| text.starts_with(prefix))
}

/// Extract user-authored string/text blocks while excluding host-generated messages.
///
/// Mixed text arrays retain typed blocks independently of injected reminder blocks.
#[must_use]
pub fn extract_prompts(records: &[Value]) -> Vec<Prompt> {
    let mut prompts = Vec::new();
    for (index, record) in records.iter().enumerate() {
        if record["type"] != "user"
            || ["isMeta", "isCompactSummary", "isSidechain"]
                .iter()
                .any(|key| record[*key].as_bool() == Some(true))
        {
            continue;
        }
        let content = &record["message"]["content"];
        let text = if let Some(text) = content.as_str() {
            if injected(text) {
                continue;
            }
            text.to_owned()
        } else if let Some(blocks) = content.as_array() {
            if blocks.iter().any(|b| b["type"] == "tool_result") {
                continue;
            }
            blocks
                .iter()
                .filter(|b| b["type"] == "text")
                .filter_map(|b| b["text"].as_str())
                .filter(|text| !injected(text))
                .collect::<Vec<_>>()
                .join("\n")
        } else {
            continue;
        };
        if text.trim().is_empty() {
            continue;
        }
        let model = records[index + 1..]
            .iter()
            .find(|r| r["type"] == "assistant")
            .and_then(|r| r["message"]["model"].as_str())
            .map(str::to_owned);
        prompts.push(Prompt { text, model });
    }
    prompts
}
