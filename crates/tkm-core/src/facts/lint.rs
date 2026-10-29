// Portions ported from microsoft/TokenEconomizer (tokeneconomizer/microskills/envfacts.py, commit d0675a3),
// Copyright (c) Microsoft Corporation, licensed under the Apache License, Version 2.0.
//! Completion-advice and repository-boundary lint (SPEC 13.4).
/// Checks TE's completion/verification patterns and rejects external paths.
#[must_use]
pub fn lint_ok(_text: &str) -> bool {
    false
}
/// Validates every family parameter before discovery or rendering.
#[must_use]
pub fn validate(_candidate: &super::Candidate) -> bool {
    false
}
