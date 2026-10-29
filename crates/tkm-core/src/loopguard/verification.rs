// Portions ported from microsoft/TokenEconomizer (tokeneconomizer/loopguard/turn.py, state.py, commit d0675a3),
// Copyright (c) Microsoft Corporation, licensed under the Apache License, Version 2.0.
//! Verification status across shell pipelines and test runner summaries.
use super::shell_edits::{
    ShellSegment, basename, executable_words, python_script, segment_edit_targets, shell_segments,
};
use crate::model::Call;

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum TestStatus {
    Own,
    Hidden,
    Mixed,
}

pub(super) fn is_shell(call: &Call) -> bool {
    matches!(
        call.tool.to_ascii_lowercase().as_str(),
        "bash" | "shell" | "exec_command" | "command_execution" | "run_shell_command" | "terminal"
    )
}

// A later check in an && chain can fail only after earlier writes succeeded.
// Keep those edits, while excluding final writes and explicitly named write errors.
pub(super) fn edit_targets(call: &Call) -> Vec<String> {
    let failed = call.is_error || call.exit_code.is_some_and(|code| code != 0);
    let mut inferred = Vec::new();
    if is_shell(call) {
        let segments = shell_segments(&call.args_digest);
        for (index, segment) in segments.iter().enumerate() {
            let targets = segment_edit_targets(segment);
            if failed
                && (named_write_failure(segment, &call.output_digest)
                    || !check_after_write(&segments, index))
            {
                continue;
            }
            for target in targets {
                if !inferred.contains(&target) {
                    inferred.push(target);
                }
            }
        }
    }
    let generic = inferred
        .iter()
        .any(|s| matches!(s.as_str(), "(patch)" | "(script)"));
    let mut names: Vec<String> = call
        .edits
        .iter()
        .filter_map(|edit| edit.path.to_str())
        .map(basename)
        .filter(|s| !s.is_empty())
        .filter(|s| !failed || generic || inferred.iter().any(|p| p == s))
        .map(str::to_owned)
        .collect();
    let known = !names.is_empty();
    for target in inferred {
        if known && matches!(target.as_str(), "(patch)" | "(script)") {
            continue;
        }
        if !names.contains(&target) {
            names.push(target);
        }
    }
    names
}

fn check_after_write(segments: &[ShellSegment], index: usize) -> bool {
    for next in index + 1..segments.len() {
        if segments[next - 1].next != "&&" {
            break;
        }
        if verifies(&segments[next]) {
            return true;
        }
    }
    false
}

fn named_write_failure(segment: &ShellSegment, output: &str) -> bool {
    let words = executable_words(&segment.words);
    let head = words.first().map(|s| basename(s)).unwrap_or_default();
    let output = output.to_ascii_lowercase();
    let prefix = format!("{head}:");
    let named = !head.is_empty()
        && output
            .lines()
            .any(|line| line.trim_start().starts_with(&prefix));
    let patch = matches!(head, "patch" | "apply_patch")
        || (head == "git" && words.get(1).is_some_and(|s| s == "apply"));
    named
        || (patch
            && [
                "patch failed",
                "patch does not apply",
                "failed to find context",
                "apply_patch: error",
            ]
            .iter()
            .any(|s| output.contains(s)))
}

pub(super) fn has_error(text: &str) -> bool {
    error_pattern().is_some_and(|re| re.is_match(text))
}

pub(super) fn error_line(text: &str) -> Option<&str> {
    text.lines().find(|line| has_error(line))
}

fn error_pattern() -> Option<&'static regex::Regex> {
    pattern!(
        r"(?im)(Traceback \(most recent call last\)|\b\w*Error\b|\bFAILED\b|\bassert\b|Exception|errors during collection|^# fail [1-9]|^not ok \d|\b[1-9]\d* (?:failing|failures?)\b|Failures: [1-9]|^(?-i:--- FAIL: |FAIL\t|FAIL$))"
    )
}

// Keep the Python-only signal vocabulary used to calibrate SpiralState, while
// ended-green classification below recognizes project checks across ecosystems.
pub(super) fn counts_test_run(command: &str) -> bool {
    let pattern = pattern!(
        r"(?i)(runtests(\.py)?|\bpytest\b|py\.test|manage\.py\s+test|python\s+-m\s+unittest|\btox\b|\bnox\b|\btrial\b|python\d?\s+-c\b)"
    );
    shell_segments(command).iter().any(|segment| {
        let text = format!(
            "{} {}",
            segment.words.join(" "),
            python_script(segment).unwrap_or_default()
        );
        pattern.is_some_and(|re| re.is_match(&text))
    })
}

pub(super) fn is_verification(command: &str) -> bool {
    status_of_tests(command).is_some()
}

pub(super) fn reported_failure(call: &Call) -> Option<bool> {
    if call.is_error {
        return Some(true);
    }
    let original = call.exit_code.map(|code| code != 0);
    if !is_shell(call) {
        return original;
    }
    match status_of_tests(&call.args_digest) {
        Some(TestStatus::Hidden) => runner_verdict(&call.output_digest),
        Some(TestStatus::Mixed) => runner_verdict(&call.output_digest).or(original),
        _ => original,
    }
}

fn status_of_tests(command: &str) -> Option<TestStatus> {
    let stages = shell_segments(command);
    let pipefail = command.contains("pipefail");
    let mut found = None;
    for (index, segment) in stages.iter().enumerate() {
        if !verifies(segment) {
            continue;
        }
        let mut status = TestStatus::Own;
        for pos in index..stages.len().saturating_sub(1) {
            let sep = stages[pos].next.as_str();
            if sep == "&&" || (pipefail && matches!(sep, "|" | "|&")) {
                continue;
            }
            if matches!(sep, "|" | "|&" | "||") {
                status = TestStatus::Hidden;
                break;
            }
            if !stages[pos + 1]
                .words
                .first()
                .is_some_and(|s| matches!(s.as_str(), "echo" | "printf" | "true" | ":" | "exit"))
            {
                status = TestStatus::Mixed;
                break;
            }
            status = TestStatus::Hidden;
        }
        found = Some(found.map_or(status, |s: TestStatus| s.max(status)));
    }
    found
}

fn verifies(segment: &ShellSegment) -> bool {
    runner(&segment.words)
        || python_script(segment).is_some_and(|script| {
            pattern!(r"\b(?:pytest|unittest)\.main\s*\(").is_some_and(|re| re.is_match(&script))
        })
}

fn runner(words: &[String]) -> bool {
    if words.iter().any(|s| {
        matches!(
            s.as_str(),
            "-h" | "--help" | "--version" | "--collect-only" | "--co"
        ) || s.starts_with("--list")
    }) {
        return false;
    }
    let words = executable_words(words);
    let Some(head) = words.first().map(|s| basename(s)) else {
        return false;
    };
    let rest = &words[1..];
    let first = rest.first().map(String::as_str).unwrap_or_default();
    match head {
        "pytest" | "py.test" | "tox" | "nox" | "runtests" | "runtests.py" | "jest" | "vitest"
        | "mocha" | "ava" | "ctest" | "rspec" | "phpunit" => true,
        "test" => words[0].ends_with("bin/test"),
        "npm" | "pnpm" | "yarn" | "bun" => {
            first == "test" || (first == "run" && rest.get(1).is_some_and(|s| s == "test"))
        }
        "cargo" => matches!(first, "test" | "nextest"),
        "make" => matches!(first, "test" | "check"),
        "go" | "dotnet" | "swift" | "mix" | "deno" | "bazel" | "playwright" | "manage.py" => {
            first == "test"
        }
        "node" => first == "--test",
        "mvn" | "mvnw" | "gradle" | "gradlew" => rest.iter().any(|s| s == "test"),
        _ if head.starts_with("python") => {
            (first == "-m"
                && rest
                    .get(1)
                    .is_some_and(|s| matches!(s.as_str(), "pytest" | "unittest")))
                || matches!(basename(first), "runtests" | "runtests.py")
                || (basename(first) == "manage.py" && rest.get(1).is_some_and(|s| s == "test"))
                || first.ends_with("bin/test")
        }
        _ => false,
    }
}

fn runner_verdict(text: &str) -> Option<bool> {
    let failed = pattern!(r"(?m)\b[1-9]\d* (?:failed|errors?)\b.*\bin \d+(?:\.\d+)?s\b|^FAILED \((?:failures|errors)=|^tests finished:.*\b[1-9]\d* (?:failed|exceptions?)\b|^# fail [1-9]|^Tests:.*\b[1-9]\d* failed\b|^\s*Tests\s+[1-9]\d* failed\b|^\s*[1-9]\d* failing\b|^(?:--- FAIL: |FAIL\t|FAIL$)|^test result: FAILED|\b\d+ examples?, [1-9]\d* failures?\b|Tests run: \d+, Failures: (?:[1-9]|0, Errors: [1-9])|BUILD FAIL(?:URE|ED)")
        .is_some_and(|re| re.is_match(text));
    if failed {
        return Some(true);
    }
    pattern!(r"(?m)\b\d+ passed\b.*\bin \d+(?:\.\d+)?s\b|^OK(?: \(.*\))?\s*$|^tests finished: \d+ passed|^# fail 0\b|^Tests:\s+\d+ passed|^\s*Tests\s+\d+ passed|^\s*\d+ passing\b|^ok\s+\S+\s+(?:\d+(?:\.\d+)?s|\(cached\))|^test result: ok\.|\b\d+ examples?, 0 failures\b|BUILD SUCCESS")
        .is_some_and(|re| re.is_match(text)).then_some(false)
}
