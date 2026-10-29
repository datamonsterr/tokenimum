// Portions ported from microsoft/TokenEconomizer (tokeneconomizer/ledger.py, commit d0675a3),
// Copyright (c) Microsoft Corporation, licensed under the Apache License, Version 2.0.
//! Pure ledger, pricing and savings estimates (SPEC 7.5).
mod pricing;
mod savings;
use crate::model::Usage;
pub use pricing::{Price, PriceTable};
pub use savings::{ConfidenceBand, SavingsEstimate, SavingsSession, estimate_savings};
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
/// Sum decision costs in US dollars.
#[must_use]
pub fn decision_overhead(decisions: &[DecisionOverhead]) -> f64 {
    decisions.iter().map(|d| d.cost).sum()
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
    /// Add one task and return its total cost.
    pub fn add(&mut self, exec: f64, overhead: f64) -> f64 {
        self.cum_exec += exec;
        self.cum_overhead += overhead;
        self.n += 1;
        exec + overhead
    }
    /// Total accumulated cost.
    #[must_use]
    pub fn cum_total(&self) -> f64 {
        self.cum_exec + self.cum_overhead
    }
    /// Average cost per task, or zero before any tasks.
    #[must_use]
    #[allow(clippy::cast_precision_loss)]
    pub fn amortized_per_task(&self) -> f64 {
        if self.n == 0 {
            0.0
        } else {
            self.cum_total() / self.n as f64
        }
    }
    /// Fraction spent on overhead, or zero when total is zero.
    #[must_use]
    pub fn overhead_frac(&self) -> f64 {
        let total = self.cum_total();
        if total == 0.0 {
            0.0
        } else {
            self.cum_overhead / total
        }
    }
}
