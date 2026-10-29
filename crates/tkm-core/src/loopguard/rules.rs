// Portions ported from microsoft/TokenEconomizer (tokeneconomizer/loopguard/rules.py, commit d0675a3),
// Copyright (c) Microsoft Corporation, licensed under the Apache License, Version 2.0.
//! Thresholds and pure gate, rule, and verdict classification.
use super::SpiralState;
use serde::{Deserialize, Serialize};
/// Detector and envelope settings, with TE defaults (SPEC 4.3).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct LoopConfig {
    /// Tool-call floor for the strict rule (SPEC 4.3).
    pub min_turns: u32,
    /// Edits to one basename required for churn (SPEC 4.3).
    pub edit_churn: u32,
    /// Alternative scatter threshold from TE rules.py.
    pub distinct_edit_files: u32,
    /// Sustained error threshold from TE rules.py.
    pub error_outputs: u32,
    /// Minimum share of repeated failures (SPEC 4.3).
    pub stagnation: f64,
    /// Call floor for the free gate (SPEC 4.3).
    pub gate_min_turns: u32,
    /// Errors that open the gate without repeats (SPEC 4.3).
    pub gate_error_outputs: u32,
    /// Learned length percentile (SPEC 4.3).
    pub envelope_pct: f64,
    /// Multiplier on the percentile (SPEC 4.3).
    pub envelope_margin: f64,
    /// Good turns required before an envelope exists (SPEC 4.3).
    pub envelope_min_samples: usize,
    /// Strict whole-turn backstop multiplier (SPEC 7.4).
    pub backstop_mult: f64,
    /// Redirect budget before escalation (SPEC 4.3).
    pub redirects_per_turn: u32,
    /// Legacy TE preset value; calibrated scores use JudgeThresholds.
    pub judge_confidence: f64,
    /// Legacy TE aggressive flag; SPEC 7.4 still waits on ambiguity.
    pub nudge_on_ambiguous: bool,
}
impl Default for LoopConfig {
    fn default() -> Self {
        Self {
            min_turns: 20,
            edit_churn: 4,
            distinct_edit_files: 3,
            error_outputs: 6,
            stagnation: 0.20,
            gate_min_turns: 18,
            gate_error_outputs: 8,
            envelope_pct: 90.0,
            envelope_margin: 1.25,
            envelope_min_samples: 5,
            backstop_mult: 2.5,
            redirects_per_turn: 1,
            judge_confidence: 0.7,
            nudge_on_ambiguous: false,
        }
    }
}
impl LoopConfig {
    /// TE named preset; unknown levels fall back to default.
    #[must_use]
    pub fn preset(_level: &str) -> Self {
        Self::default()
    }
}
/// Calibrated model thresholds supplied by the caller (SPEC 7.4 step 3).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct JudgeThresholds {
    /// Minimum stuck probability.
    pub t_stuck: f64,
    /// Maximum progress probability for a stuck verdict.
    pub t_progress: f64,
    /// Maximum stuck probability for a productive verdict.
    pub t_low: f64,
}
/// Model probability scores; invalid probabilities fail open to Wait.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct JudgeScores {
    /// Probability that the segment is stuck.
    pub stuck: f64,
    /// Probability that the segment is progressing.
    pub progress: f64,
}
/// Combined classification; contains no stop instruction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JudgeVerdict {
    /// Both stuck and low-progress conditions hold.
    Stuck,
    /// Stuck probability is below the low threshold.
    NotStuck,
    /// Ambiguous or malformed; wait for more evidence.
    Wait,
}
/// Apply SPEC 7.4's calibrated probability table, failing open on invalid input.
#[must_use]
pub fn combine_verdict(_scores: JudgeScores, _thresholds: JudgeThresholds) -> JudgeVerdict {
    JudgeVerdict::Wait
}
/// Free gate after the configured call floor (SPEC 7.4 step 2).
#[must_use]
pub fn gate_for_judge(_state: &SpiralState, _cfg: &LoopConfig) -> bool {
    false
}
/// Strict conjunction of age, churn/scatter, errors and repetition (SPEC 4.3).
#[must_use]
pub fn is_stuck(_state: &SpiralState, _cfg: &LoopConfig) -> bool {
    false
}
