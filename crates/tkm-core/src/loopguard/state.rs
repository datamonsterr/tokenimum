// Portions ported from microsoft/TokenEconomizer (tokeneconomizer/loopguard/state.py, commit d0675a3),
// Copyright (c) Microsoft Corporation, licensed under the Apache License, Version 2.0.
//! Compact per-call spiral signals.
use crate::model::Call;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
/// Serializable counts for a segment or a whole prompt-turn.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct SpiralState {
    /// Completed calls, including non-failing content reads.
    pub turns: u32,
    /// Edits per basename, matching TE's signal unit.
    pub edit_counts: BTreeMap<String, u32>,
    /// Ordered edit basenames.
    pub edit_order: Vec<String>,
    /// Outputs containing error markers and eligible to fail.
    pub error_outputs: u32,
    /// Verification commands seen.
    pub test_runs: u32,
    /// Stable normalized failure fingerprints and counts; never raw output.
    pub fail_sigs: BTreeMap<String, u32>,
    /// Failures repeating a previous fingerprint.
    pub repeated_fail_sigs: u32,
}
impl SpiralState {
    /// Fold a normalized call once; shell edits and normalized edits are deduplicated.
    pub fn update(&mut self, _call: &Call) {}
    /// Maximum edits to a basename.
    #[must_use]
    pub fn edit_churn(&self) -> u32 {
        0
    }
    /// Number of edited basenames.
    #[must_use]
    pub fn distinct_edit_files(&self) -> u32 {
        0
    }
    /// Fraction of failing outputs repeating a previous failure.
    #[must_use]
    pub fn stagnation(&self) -> f64 {
        0.0
    }
}
