//! Structured decision digest.
//!
//! The plain-text format contains a prompt, chronological call lines, edit
//! counts, error and repeat counts, and optional test totals. Calls are dropped
//! oldest first when needed to fit the byte limit.
use crate::redact::{Redactor, is_dotenv_path};
use std::collections::{BTreeMap, HashSet};
use std::fmt::Write;
use std::path::Path;
use tkm_core::model::Call;
/// Maximum digest length in bytes.
pub const MAX_DIGEST_BYTES: usize = 16_384;
/// Test results included in a digest.
#[derive(Debug, Clone, Default)]
pub struct TestSummary {
    /// Passed tests.
    pub passed: usize,
    /// Failed tests.
    pub failed: usize,
}
/// Source material for a digest.
#[allow(clippy::module_name_repetitions)] // Public name identifies the digest builder input.
pub struct DigestInput<'a> {
    /// User prompt.
    pub prompt: &'a str,
    /// Calls in chronological order.
    pub calls: &'a [Call],
    /// Optional test results.
    pub tests: Option<TestSummary>,
}
/// Builds a bounded, redacted digest.
#[must_use]
pub fn build_digest(input: DigestInput<'_>, redactor: &Redactor) -> String {
    let prompt: String = redactor.redact(input.prompt).chars().take(500).collect();
    let calls: Vec<_> = input
        .calls
        .iter()
        .rev()
        .take(12)
        .rev()
        .map(|call| call_line(call, redactor))
        .collect();
    let mut edits = BTreeMap::new();
    let mut seen = HashSet::new();
    let mut repeats = 0usize;
    for call in input.calls {
        if !seen.insert((&call.tool, &call.args_digest)) {
            repeats += 1;
        }
        for edit in &call.edits {
            *edits
                .entry(redactor.redact(&edit.path.to_string_lossy()))
                .or_insert(0usize) += 1;
        }
    }
    let mut tail = format!(
        "errors: {}\nrepeats: {repeats}/{}\nedits:\n",
        input.calls.iter().filter(|c| c.is_error).count(),
        input.calls.len()
    );
    let remaining = edits.len().saturating_sub(50);
    for (path, count) in edits.into_iter().take(50) {
        let _ = writeln!(tail, "{path}: {count}");
    }
    if remaining > 0 {
        let _ = writeln!(tail, "... {remaining} more files");
    }
    if let Some(tests) = input.tests {
        let _ = writeln!(
            tail,
            "tests: {} passed, {} failed",
            tests.passed, tests.failed
        );
    }
    let head = format!("prompt: {prompt}\ncalls:\n");
    let mut start = 0;
    while start < calls.len()
        && head
            .len()
            .saturating_add(tail.len())
            .saturating_add(calls[start..].iter().map(String::len).sum::<usize>())
            > MAX_DIGEST_BYTES
    {
        start += 1;
    }
    let mut output = head;
    for call in &calls[start..] {
        output.push_str(call);
    }
    output.push_str(&tail);
    if output.len() > MAX_DIGEST_BYTES {
        let mut end = MAX_DIGEST_BYTES;
        while !output.is_char_boundary(end) {
            end -= 1;
        }
        output.truncate(end);
    }
    output
}

fn call_line(call: &Call, redactor: &Redactor) -> String {
    let touches_dotenv = call.edits.iter().any(|edit| is_dotenv_path(&edit.path))
        || call
            .args_digest
            .split(|c: char| {
                c.is_whitespace()
                    || matches!(
                        c,
                        '"' | '\'' | ',' | ':' | '=' | ';' | '>' | '<' | '|' | '&' | '`'
                    )
            })
            .any(|part| is_dotenv_path(Path::new(part)));
    let args_short: String = if touches_dotenv {
        "[dotenv]".into()
    } else {
        redactor
            .redact(&call.args_digest)
            .chars()
            .take(120)
            .collect()
    };
    let error: String = if call.is_error && !touches_dotenv {
        redactor
            .redact(&call.output_digest)
            .lines()
            .next()
            .unwrap_or_default()
            .chars()
            .take(120)
            .collect()
    } else {
        String::new()
    };
    let tool: String = redactor.redact(&call.tool).chars().take(64).collect();
    let exit = call
        .exit_code
        .as_ref()
        .map_or_else(|| "?".to_owned(), ToString::to_string);
    format!("{tool} | {args_short} | {exit} | {error}\n")
}
