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
    let mut calls = Vec::new();
    for call in input.calls.iter().rev().take(12).rev() {
        let args = redactor.redact(&call.args_digest);
        let args_short: String = args.chars().take(120).collect();
        let touches_dotenv = call.edits.iter().any(|e| is_dotenv_path(&e.path))
            || call
                .args_digest
                .split(|c: char| {
                    c.is_whitespace() || matches!(c, '"' | '\'' | ',' | ':' | '=' | ';')
                })
                .any(|part| is_dotenv_path(Path::new(part)));
        let error = if call.is_error && !touches_dotenv {
            redactor
                .redact(&call.output_digest)
                .lines()
                .next()
                .unwrap_or_default()
                .to_owned()
        } else {
            String::new()
        };
        calls.push(format!(
            "{} | {args_short} | {} | {error}\n",
            redactor.redact(&call.tool),
            call.exit_code
                .map_or_else(|| "?".to_owned(), |code| code.to_string())
        ));
    }
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
    for (path, count) in edits {
        let _ = writeln!(tail, "{path}: {count}");
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
