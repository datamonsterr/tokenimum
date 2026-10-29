// Portions ported from microsoft/TokenEconomizer (tokeneconomizer/loopguard/envelope.py, turn.py, commit d0675a3),
// Copyright (c) Microsoft Corporation, licensed under the Apache License, Version 2.0.
//! Good-turn classification and the bounded repository baseline (SPEC 7.4).
use super::LoopConfig;
use crate::model::Turn;
use serde::{Deserialize, Serialize};
/// The most recent 500 good-turn lengths, in tool calls (SPEC 7.4 step 7).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct EnvelopeTracker {
    lengths: Vec<u32>,
}
impl EnvelopeTracker {
    /// Record a classified good, nonempty turn; returns whether it was included.
    pub fn record(&mut self, _turn: &Turn, _escalated: bool) -> bool {
        false
    }
    /// Retained good-turn lengths, oldest first.
    #[must_use]
    pub fn lengths(&self) -> &[u32] {
        &self.lengths
    }
    /// TE's linearly interpolated percentile times margin; None during warm-up.
    #[must_use]
    pub fn value(&self, _cfg: &LoopConfig) -> Option<f64> {
        None
    }
}
/// Own stop, nonempty, not escalated, and no final failing check after the last edit.
#[must_use]
pub fn is_good_turn(_turn: &Turn, _escalated: bool) -> bool {
    false
}
