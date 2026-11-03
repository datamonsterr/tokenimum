// Portions ported from microsoft/TokenEconomizer (tokeneconomizer/loopguard/envelope.py, turn.py, commit d0675a3),
// Copyright (c) Microsoft Corporation, licensed under the Apache License, Version 2.0.
//! Good-turn classification and the bounded repository baseline (SPEC 7.4).
use super::LoopConfig;
use super::verification;
use crate::model::{StopReason, Turn};
use serde::{Deserialize, Serialize};
/// The most recent 500 good-turn lengths, in tool calls (SPEC 7.4 step 7).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct EnvelopeTracker {
    lengths: Vec<u32>,
}
impl EnvelopeTracker {
    /// Rebuild a tracker from stored good-turn lengths, oldest first; empty turns are skipped.
    #[must_use]
    pub fn from_lengths(lengths: &[u32]) -> Self {
        let mut lengths: Vec<u32> = lengths.iter().copied().filter(|n| *n > 0).collect();
        if lengths.len() > 500 {
            lengths.drain(..lengths.len() - 500);
        }
        Self { lengths }
    }
    /// Record a classified good, nonempty turn; returns whether it was included.
    pub fn record(&mut self, turn: &Turn, escalated: bool) -> bool {
        if !is_good_turn(turn, escalated) {
            return false;
        }
        self.lengths
            .push(u32::try_from(turn.calls.len()).unwrap_or(u32::MAX));
        if self.lengths.len() > 500 {
            self.lengths.drain(..self.lengths.len() - 500);
        }
        true
    }
    /// Retained good-turn lengths, oldest first.
    #[must_use]
    pub fn lengths(&self) -> &[u32] {
        &self.lengths
    }
    /// TE's linearly interpolated percentile times margin; None during warm-up.
    #[must_use]
    pub fn value(&self, cfg: &LoopConfig) -> Option<f64> {
        if self.lengths.is_empty()
            || self.lengths.len() < cfg.envelope_min_samples
            || !cfg.envelope_pct.is_finite()
            || !(0.0..=100.0).contains(&cfg.envelope_pct)
            || !cfg.envelope_margin.is_finite()
            || cfg.envelope_margin <= 0.0
        {
            return None;
        }
        let mut sorted = self.lengths.clone();
        sorted.sort_unstable();
        let last = u32::try_from(sorted.len() - 1).ok()?;
        let index = cfg.envelope_pct / 100.0 * f64::from(last);
        // Search the (at most 500) integer ranks rather than casting a float to usize.
        let lo = (0..=last).take_while(|n| f64::from(*n) <= index).last()?;
        let lower = f64::from(*sorted.get(usize::try_from(lo).ok()?)?);
        let upper = f64::from(*sorted.get(usize::try_from((lo + 1).min(last)).ok()?)?);
        let result = (lower + (index - f64::from(lo)) * (upper - lower)) * cfg.envelope_margin;
        (result.is_finite() && result > 0.0).then_some(result)
    }
}
/// Own stop, nonempty, not escalated, and no final failing check after the last edit.
#[must_use]
pub fn is_good_turn(turn: &Turn, escalated: bool) -> bool {
    if turn.stopped_by != StopReason::OwnStop || escalated || turn.calls.is_empty() {
        return false;
    }
    let mut failed_after_edit = false;
    for call in &turn.calls {
        let shell = verification::is_shell(call);
        if !verification::edit_targets(call).is_empty() {
            failed_after_edit = false;
        }
        // A compound edit-and-test counts as verifying that edit, as in TE.
        if shell && verification::is_verification(&call.args_digest) {
            failed_after_edit = verification::reported_failure(call)
                .unwrap_or_else(|| verification::has_error(&call.output_digest));
        }
    }
    !failed_after_edit
}
