// Portions ported from microsoft/TokenEconomizer (tokeneconomizer/microskills/envfacts.py, commit d0675a3),
// Copyright (c) Microsoft Corporation, licensed under the Apache License, Version 2.0.
//! Code-owned templates and bounded fact blocks.
use super::{Candidate, FactsConfig, FactsState};
use crate::model::Host;
/// Header is a code literal and counts toward the token cap.
pub const HEADER: &str = "# environment facts learned from earlier sessions in this repository";
/// Renders a validated family claim, or nothing when parameters are unsafe/incomplete.
#[must_use]
pub fn claim(_candidate: &Candidate) -> String {
    String::new()
}
/// Renders eligible facts for a harness, merging equivalent repairs and enforcing caps.
#[must_use]
pub fn render(_state: &FactsState, _host: Host, _cfg: &FactsConfig) -> String {
    String::new()
}
