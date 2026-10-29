// Portions ported from microsoft/TokenEconomizer (tokeneconomizer/microskills/envfacts.py, commit d0675a3),
// Copyright (c) Microsoft Corporation, licensed under the Apache License, Version 2.0.
//! Completion-advice and repository-boundary lint (SPEC 13.4).

use super::{Candidate, Family, LabelRule, observe};
/// Checks TE's completion/verification patterns and rejects external paths.
///
/// SPEC 13.4 rejects absolute POSIX/Windows paths, home expansion, and parent
/// components. Relative paths are lexical; symlink resolution belongs to readers.
#[must_use]
pub fn lint_ok(text: &str) -> bool {
    !observe::matches(
        r"(?i)\b(skip|no need|don'?t|do not|stop|done|completed?|finish(?:ed)?|verify|verification|instead of (?:running|testing))\b",
        text,
    ) && !observe::matches(r#"(?:\A|[\s`'"(=])(?:/|~|[A-Za-z]:[/\\]|\\)"#, text)
        && !observe::matches(r#"(?:\A|[\s/\\`'"(=])\.\.(?:\z|[\s/\\`'"),])"#, text)
        && !text.contains('~')
}
fn label(value: &str) -> bool {
    observe::matches(
        r"\A[A-Za-z_][A-Za-z0-9_.\-/]*(?:::[A-Za-z0-9_.\-\[\]]+)*\z",
        value,
    ) && lint_ok(value)
}
fn host(value: &str) -> bool {
    value.len() <= 253
        && observe::matches(
            r"\A[A-Za-z0-9](?:[A-Za-z0-9-]{0,61}[A-Za-z0-9])?(?:\.[A-Za-z0-9](?:[A-Za-z0-9-]{0,61}[A-Za-z0-9])?)*\z",
            value,
        )
}
fn command(value: &str) -> bool {
    if !observe::matches(
        r"\A[A-Za-z0-9_./+:-]+(?: [A-Za-z0-9_./+:-]+| && [A-Za-z0-9_./+:-]+)*\z",
        value,
    ) || !lint_ok(value)
    {
        return false;
    }
    let classified = observe::classify_command(value);
    classified.len() == 1 && classified[0].form == value
}
/// Validates every family parameter before discovery or rendering.
///
/// Only command forms, uppercase env names, host names, and test-label-shaped
/// settings/examples are accepted. Error classes and probe kinds are closed enums.
/// Deserialized state crosses this validation boundary again at render time.
#[must_use]
pub fn validate(candidate: &Candidate) -> bool {
    let p = &candidate.params;
    let all_forms = std::iter::once(p.bad_form.as_str())
        .chain(std::iter::once(p.form.as_str()))
        .chain(p.forms.keys().map(String::as_str))
        .chain(p.form_failures.keys().map(String::as_str));
    if !all_forms.filter(|s| !s.is_empty()).all(command)
        || !p.hosts.keys().all(|s| host(s))
        || !p
            .examples
            .iter()
            .all(|(bad, good)| label(bad) && label(good))
        || !p.good.keys().chain(p.bad.keys()).all(|s| label(s))
        || (!p.variable.is_empty() && !observe::matches(r"\A[A-Z_][A-Z0-9_]*\z", &p.variable))
        || p.rule
            .as_ref()
            .is_some_and(|r| matches!(r,LabelRule::Strip(s) if !label(s)))
    {
        return false;
    }
    match candidate.family {
        Family::FormRepair => {
            !p.bad_form.is_empty()
                && matches!(
                    p.intent,
                    Some(
                        observe::Intent::RunTests
                            | observe::Intent::RunBuild
                            | observe::Intent::RunInterpreter
                            | observe::Intent::PackageTool
                    )
                )
                && !p.errors.is_empty()
                && p.errors.keys().all(|e| !observe::network(*e))
        }
        Family::Boundary => !p.hosts.is_empty(),
        Family::EmptyProbe => !p.probes.is_empty(),
        Family::ArgumentRepair => !p.form.is_empty() && p.rule.is_some() && !p.examples.is_empty(),
        Family::EnvValue => !p.variable.is_empty() && !p.good.is_empty(),
    }
}
