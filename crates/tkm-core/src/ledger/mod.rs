//! Session costs and savings.
use crate::model::Usage;
use serde::{Deserialize, Serialize};
/// Typed overhead fields in the TokenEconomizer ledger.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct Overhead {
    /// Recorded optimizer tokens.
    pub optimizer_tokens: f64,
    /// Recorded untool classify tokens.
    pub untool_classify_tokens: f64,
    /// Recorded untool judge tokens.
    pub untool_judge_tokens: f64,
    /// Recorded codex untool adapt tokens.
    pub codex_untool_adapt_tokens: f64,
    /// Recorded spiral judge tokens.
    pub spiral_judge_tokens: f64,
    /// Recorded spiral nudge writer tokens.
    pub spiral_nudge_writer_tokens: f64,
}
/// Sum known overhead fields.
pub fn overhead(_: &Overhead) -> f64 {
    0.0
}
/// Claude effective execution tokens, or total when no breakdown is available.
pub fn effective_exec(_: Option<Usage>, _: f64) -> f64 {
    0.0
}
/// Accumulates execution and overhead costs in the same unit.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct CostLedger {
    /// Recorded cum exec.
    pub cum_exec: f64,
    /// Recorded cum overhead.
    pub cum_overhead: f64,
    /// Recorded n.
    pub n: u64,
}
impl CostLedger {
    /// Add one task.
    pub fn add(&mut self, _: f64, _: f64) -> f64 {
        0.0
    }
    /// Total accumulated cost.
    pub fn cum_total(&self) -> f64 {
        0.0
    }
    /// Average cost per task.
    pub fn amortized_per_task(&self) -> f64 {
        0.0
    }
    /// Fraction spent on overhead.
    pub fn overhead_frac(&self) -> f64 {
        0.0
    }
}
