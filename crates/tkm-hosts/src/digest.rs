//! Deterministic call descriptions shared by live mapping and transcript readers.
//!
//! These descriptions are capped at 512 Unicode scalar values and are not
//! redacted. External decision clients must redact them before transmission.

use serde_json::Value;

/// Truncates text without splitting a UTF-8 character.
#[must_use]
pub fn truncate(text: &str, limit: usize) -> String {
    text.chars().take(limit).collect()
}

/// Describes shell commands directly, or other input as sorted compact JSON.
#[must_use]
pub fn args_digest(tool: &str, input: &Value) -> String {
    let text = if matches!(
        tool,
        "Bash" | "bash" | "shell" | "shell_command" | "exec_command"
    ) {
        input
            .get("command")
            .or_else(|| input.get("cmd"))
            .and_then(Value::as_str)
            .map(str::to_owned)
    } else {
        None
    };
    truncate(&text.unwrap_or_else(|| canonical_json(input)), 512)
}

fn canonical_json(value: &Value) -> String {
    match value {
        Value::Object(object) => {
            let mut keys: Vec<_> = object.keys().collect();
            keys.sort_unstable();
            let entries: Vec<_> = keys
                .into_iter()
                .map(|key| {
                    format!(
                        "{}:{}",
                        Value::String(key.clone()),
                        canonical_json(&object[key])
                    )
                })
                .collect();
            format!("{{{}}}", entries.join(","))
        }
        Value::Array(values) => format!(
            "[{}]",
            values
                .iter()
                .map(canonical_json)
                .collect::<Vec<_>>()
                .join(",")
        ),
        _ => value.to_string(),
    }
}

/// Extracts the textual content of common tool-result shapes.
///
/// Strings and text blocks retain their content; shell stdout precedes stderr.
/// Other structured values use deterministic compact JSON.
#[must_use]
pub fn output_text(output: &Value) -> String {
    match output {
        Value::Null => String::new(),
        Value::String(text) => text.clone(),
        Value::Array(blocks) => blocks
            .iter()
            .map(output_text)
            .filter(|text| !text.is_empty())
            .collect::<Vec<_>>()
            .join("\n"),
        Value::Object(object) => {
            if object.contains_key("stdout") || object.contains_key("stderr") {
                ["stdout", "stderr"]
                    .into_iter()
                    .filter_map(|key| object.get(key).and_then(Value::as_str))
                    .filter(|text| !text.is_empty())
                    .collect::<Vec<_>>()
                    .join("\n")
            } else if let Some(text) = object.get("text").and_then(Value::as_str) {
                text.to_owned()
            } else if let Some(content) = object.get("content").or_else(|| object.get("output")) {
                output_text(content)
            } else if let Some(content) = object.get("file").and_then(|file| file.get("content")) {
                output_text(content)
            } else {
                canonical_json(output)
            }
        }
        _ => canonical_json(output),
    }
}

/// Keeps the first error-marker line, otherwise the last non-empty output line.
#[must_use]
pub fn output_digest(output: &Value) -> String {
    let text = output_text(output);
    let line = text
        .lines()
        .find(|line| {
            ["error", "Error", "Exit code", "fatal", "panicked"]
                .iter()
                .any(|marker| line.starts_with(marker))
        })
        .or_else(|| text.lines().rev().find(|line| !line.trim().is_empty()))
        .unwrap_or_default();
    truncate(line, 512)
}
