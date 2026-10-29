//! Private copy of the ADR 0011 digest rules.
// TODO(e5a): replace with crate::digest after merge

use serde_json::{Map, Value};

const MAX_DIGEST_CHARS: usize = 512;
const ERROR_MARKERS: [&str; 5] = ["error", "Error", "Exit code", "fatal", "panicked"];

/// Compact JSON of `value` with object keys sorted at every level.
fn canonical(value: &Value) -> Value {
    match value {
        Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            let mut sorted = Map::new();
            for key in keys {
                if let Some(inner) = map.get(key) {
                    sorted.insert(key.clone(), canonical(inner));
                }
            }
            Value::Object(sorted)
        }
        Value::Array(items) => Value::Array(items.iter().map(canonical).collect()),
        other => other.clone(),
    }
}

fn truncate(text: &str) -> String {
    text.chars().take(MAX_DIGEST_CHARS).collect()
}

/// Digest of a shell command string.
pub(crate) fn command_digest(command: &str) -> String {
    truncate(command)
}

/// Digest of tool input: the command for shell tools, else sorted compact JSON.
pub(crate) fn args_digest(shell: bool, input: &Value) -> String {
    if shell {
        if let Some(command) = input.get("command").and_then(Value::as_str) {
            return command_digest(command);
        }
    }
    if input.is_null() {
        return String::new();
    }
    truncate(&canonical(input).to_string())
}

/// Digest of tool output: first error-marker line, else last non-empty line.
pub(crate) fn output_digest(output: &str) -> String {
    let marked = output.lines().find(|line| {
        ERROR_MARKERS
            .iter()
            .any(|m| line.trim_start().starts_with(m))
    });
    let line = marked.or_else(|| output.lines().rev().find(|line| !line.trim().is_empty()));
    truncate(line.map_or("", str::trim))
}
