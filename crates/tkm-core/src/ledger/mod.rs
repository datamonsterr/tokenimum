// Portions ported from microsoft/TokenEconomizer (tokeneconomizer/ledger.py, commit d0675a3),
// Copyright (c) Microsoft Corporation, licensed under the Apache License, Version 2.0.
//! Pure ledger, pricing and savings estimates (SPEC 7.5).
mod pricing;
mod savings;
use crate::model::Usage;
pub use pricing::{Price, PriceError, PriceTable};
pub use savings::{
    ConfidenceBand, SavingsEstimate, SavingsSession, checked_estimate_savings, estimate_savings,
};
use serde::{Deserialize, Serialize};
/// Typed token overhead fields from the reference ledger.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct Overhead {
    /// Optimizer tokens.
    pub optimizer_tokens: f64,
    /// Tool classifier tokens.
    pub untool_classify_tokens: f64,
    /// Tool judge tokens.
    pub untool_judge_tokens: f64,
    /// Codex adaptation tokens.
    pub codex_untool_adapt_tokens: f64,
    /// Spiral judge tokens.
    pub spiral_judge_tokens: f64,
    /// Nudge writer tokens.
    pub spiral_nudge_writer_tokens: f64,
}
/// Sum the six known overhead fields; other fields cannot enter the typed record.
#[must_use]
pub fn overhead(x: &Overhead) -> f64 {
    x.optimizer_tokens
        + x.untool_classify_tokens
        + x.untool_judge_tokens
        + x.codex_untool_adapt_tokens
        + x.spiral_judge_tokens
        + x.spiral_nudge_writer_tokens
}
/// Claude effective execution in fresh-input equivalents; use total without a breakdown.
#[must_use]
#[allow(clippy::cast_precision_loss)]
pub fn effective_exec(usage: Option<Usage>, total: f64) -> f64 {
    usage.map_or(total, |u| {
        u.input as f64
            + 0.1 * u.cache_read as f64
            + 1.25 * u.cache_write as f64
            + 5.0 * u.output as f64
    })
}
/// Per-decision usage and price (SPEC 7.5).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DecisionOverhead {
    /// Input tokens used.
    pub input_tokens: u64,
    /// Charged US dollars.
    pub cost: f64,
    /// Model identifier.
    pub model: String,
    /// Decision latency in milliseconds.
    pub latency_ms: u64,
    /// Names of questions asked.
    pub question_names: Vec<String>,
}
/// Sum decision costs in US dollars, rejecting invalid or overflowing totals.
#[must_use]
pub fn decision_overhead(decisions: &[DecisionOverhead]) -> Option<f64> {
    decisions.iter().try_fold(0.0, |sum, d| {
        if !valid_cost(d.cost) {
            return None;
        }
        let total = sum + d.cost;
        valid_cost(total).then_some(total)
    })
}

pub(crate) fn valid_cost(value: f64) -> bool {
    value.is_finite() && value >= 0.0
}
/// Accumulator for costs expressed in the same unit, such as US dollars.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct CostLedger {
    /// Cumulative execution cost.
    pub cum_exec: f64,
    /// Cumulative overhead cost.
    pub cum_overhead: f64,
    /// Number of tasks.
    pub n: u64,
}
impl CostLedger {
    /// Add one task atomically, returning `None` for an invalid cost, overflow,
    /// or when the task count is already `u64::MAX`.
    pub fn add(&mut self, exec: f64, overhead: f64) -> Option<f64> {
        let n = self.n.checked_add(1)?;
        if ![exec, overhead, self.cum_exec, self.cum_overhead]
            .into_iter()
            .all(valid_cost)
        {
            return None;
        }
        let new_exec = self.cum_exec + exec;
        let new_overhead = self.cum_overhead + overhead;
        let task = exec + overhead;
        if ![new_exec, new_overhead, task, new_exec + new_overhead]
            .into_iter()
            .all(valid_cost)
        {
            return None;
        }
        self.cum_exec = new_exec;
        self.cum_overhead = new_overhead;
        self.n = n;
        Some(task)
    }
    /// Checked accumulation; equivalent to [`Self::add`].
    pub fn checked_add(&mut self, exec: f64, overhead: f64) -> Option<f64> {
        self.add(exec, overhead)
    }
    /// Total accumulated cost, or `None` for invalid or overflowing state.
    #[must_use]
    pub fn cum_total(&self) -> Option<f64> {
        if !valid_cost(self.cum_exec) || !valid_cost(self.cum_overhead) {
            return None;
        }
        let total = self.cum_exec + self.cum_overhead;
        valid_cost(total).then_some(total)
    }
    /// Average cost per task, or zero before any tasks; invalid state is `None`.
    #[must_use]
    #[allow(clippy::cast_precision_loss)]
    pub fn amortized_per_task(&self) -> Option<f64> {
        let total = self.cum_total()?;
        Some(if self.n == 0 {
            0.0
        } else {
            total / self.n as f64
        })
    }
    /// Fraction spent on overhead, or zero when total is zero; invalid state is `None`.
    #[must_use]
    pub fn overhead_frac(&self) -> Option<f64> {
        let total = self.cum_total()?;
        Some(if total == 0.0 {
            0.0
        } else {
            self.cum_overhead / total
        })
    }
}

/// Normalize known numeric fields in a TokenEconomizer JSON row; null, strings,
/// missing keys and other nonnumeric values contribute zero.
#[must_use]
pub fn overhead_from_json(row: &serde_json::Value) -> f64 {
    [
        "optimizer_tokens",
        "untool_classify_tokens",
        "untool_judge_tokens",
        "codex_untool_adapt_tokens",
        "spiral_judge_tokens",
        "spiral_nudge_writer_tokens",
    ]
    .iter()
    .filter_map(|key| row.get(*key).and_then(serde_json::Value::as_f64))
    .sum()
}
/// Validate and sum decision costs.
#[must_use]
pub fn checked_decision_overhead(decisions: &[DecisionOverhead]) -> Option<f64> {
    decision_overhead(decisions)
}
