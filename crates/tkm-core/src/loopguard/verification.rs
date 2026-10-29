// Portions ported from microsoft/TokenEconomizer (tokeneconomizer/loopguard/turn.py, state.py, commit d0675a3),
// Copyright (c) Microsoft Corporation, licensed under the Apache License, Version 2.0.
//! Verification status across shell pipelines and test runner summaries.
use super::shell_edits::{basename, shell_script, tokens};
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

// A failed write attempt is not evidence that a file changed. Unknown status
// retains TE's heuristic, while either explicit failure signal rules it out.
pub(super) fn writes_allowed(call: &Call) -> bool {
    !call.is_error && call.exit_code.is_none_or(|code| code == 0)
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
    pattern!(r"(?i)(runtests(\.py)?|\bpytest\b|py\.test|manage\.py\s+test|python\s+-m\s+unittest|\btox\b|\bnox\b|\btrial\b|python\d?\s+-c\b)")
        .is_some_and(|re| re.is_match(command))
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
    let script = shell_script(command);
    let parts = tokens(&script)?;
    let mut stages: Vec<(Vec<String>, String)> = Vec::new();
    let mut words = Vec::new();
    for part in parts {
        if part.operator && matches!(part.text.as_str(), "&&" | "||" | "|" | "|&" | ";" | "\n") {
            stages.push((std::mem::take(&mut words), part.text));
        } else if !part.operator {
            words.push(part.text);
        }
    }
    if !words.is_empty() {
        stages.push((words, String::new()));
    }
    let pipefail = script.contains("pipefail");
    let mut found = None;
    for (index, (words, _)) in stages.iter().enumerate() {
        if !runner(words) {
            continue;
        }
        let mut status = TestStatus::Own;
        for pos in index..stages.len().saturating_sub(1) {
            let sep = stages[pos].1.as_str();
            if sep == "&&" || (pipefail && matches!(sep, "|" | "|&")) {
                continue;
            }
            if matches!(sep, "|" | "|&" | "||") {
                status = TestStatus::Hidden;
                break;
            }
            if !stages[pos + 1]
                .0
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

fn runner(words: &[String]) -> bool {
    if words.iter().any(|s| {
        matches!(
            s.as_str(),
            "-h" | "--help" | "--version" | "--collect-only" | "--co"
        ) || s.starts_with("--list")
    }) {
        return false;
    }
    let mut start = 0;
    while let Some(word) = words.get(start) {
        if word.contains('=')
            || matches!(
                word.as_str(),
                "time" | "nice" | "nohup" | "command" | "env" | "rtk" | "proxy"
            )
        {
            start += 1;
        } else if word == "timeout" {
            start += 1;
            while words.get(start).is_some_and(|s| {
                s.starts_with('-') || s.chars().next().is_some_and(|c| c.is_ascii_digit())
            }) {
                start += 1;
            }
        } else {
            break;
        }
    }
    let Some(head) = words.get(start).map(|s| basename(s)) else {
        return false;
    };
    let rest = &words[start + 1..];
    let first = rest.first().map(String::as_str).unwrap_or_default();
    match head {
        "pytest" | "py.test" | "tox" | "nox" | "runtests" | "runtests.py" | "jest" | "vitest"
        | "mocha" | "ava" | "ctest" | "rspec" | "phpunit" => true,
        "test" => words[start].ends_with("bin/test"),
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
