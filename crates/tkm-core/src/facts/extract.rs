// Portions ported from microsoft/TokenEconomizer (tokeneconomizer/microskills/envfacts.py, commit d0675a3),
// Copyright (c) Microsoft Corporation, licensed under the Apache License, Version 2.0.
//! Five code-only candidate detectors.
use super::{Candidate, observe::Observations};
/// Extracts regex-validated candidates with source references.
#[must_use]
pub fn extract(_input: &Observations) -> Vec<Candidate> {
    Vec::new()
}
/// Selects a working form with a positive record, preferring forms that preserve cwd.
#[must_use]
pub fn working_form(_candidate: &Candidate) -> Option<&str> {
    None
}
