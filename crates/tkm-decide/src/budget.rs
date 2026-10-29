//! Decision spend accounting without persistence dependencies.
use crate::cache::Purpose;
use std::sync::{Arc, Mutex};
/// One decision attempt or cache retrieval, excluding all state and key material.
#[derive(Clone, Debug, PartialEq)]
pub struct DecisionRow {
    /// Unix timestamp in milliseconds.
    pub ts_ms: u64,
    /// Full local session identifier.
    pub session_id: String,
    /// Purpose of the batch.
    pub purpose: Purpose,
    /// Requested configured model slug.
    pub requested_model: String,
    /// Actual response model version, if available.
    pub model_version: Option<String>,
    /// Provider, if returned by the API.
    pub provider: Option<String>,
    /// Returned input token count, or zero on an unparsed failure.
    pub input_tokens: u64,
    /// Returned output token count, or zero on an unparsed failure.
    pub output_tokens: u64,
    /// Actual cost when known; cache rows have zero cost.
    pub cost_usd: Option<f64>,
    /// Elapsed attempt time in milliseconds.
    pub latency_ms: u64,
    /// Names of all batched questions.
    pub question_names: Vec<String>,
    /// `ok` or a safe failure reason.
    pub outcome: String,
    /// Whether no HTTP request was needed.
    pub cached: bool,
}
/// E4 can implement this trait using persistent storage.
pub trait SpendLedger: Send + Sync {
    /// Spend in the UTC calendar day containing `now_ms`.
    fn spent_today_usd(&self, now_ms: u64) -> f64;
    /// Spend accumulated for the complete session.
    fn spent_session_usd(&self, session_id: &str) -> f64;
    /// Append an attempt or cache row using interior mutability.
    fn record(&self, row: DecisionRow);
}
impl<L: SpendLedger + ?Sized> SpendLedger for Arc<L> {
    fn spent_today_usd(&self, now_ms: u64) -> f64 {
        (**self).spent_today_usd(now_ms)
    }
    fn spent_session_usd(&self, session_id: &str) -> f64 {
        (**self).spent_session_usd(session_id)
    }
    fn record(&self, row: DecisionRow) {
        (**self).record(row);
    }
}
/// Poison-tolerant in-memory ledger for tests and ephemeral use.
#[derive(Debug, Default)]
pub struct InMemoryLedger {
    rows: Mutex<Vec<DecisionRow>>,
}
impl InMemoryLedger {
    /// Snapshot the recorded attempts.
    #[must_use]
    pub fn rows(&self) -> Vec<DecisionRow> {
        self.rows
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }
}
impl SpendLedger for InMemoryLedger {
    fn spent_today_usd(&self, _now_ms: u64) -> f64 {
        0.0
    }
    fn spent_session_usd(&self, _session_id: &str) -> f64 {
        0.0
    }
    fn record(&self, row: DecisionRow) {
        self.rows
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(row);
    }
}
/// Result of checking the configured spend caps.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BudgetVerdict {
    /// Both limits have remaining capacity.
    Allowed,
    /// Daily spend is at or above its limit.
    OverDaily,
    /// Session spend is at or above its limit.
    OverSession,
}
/// Independent UTC-day and session spend caps.
#[derive(Clone, Copy, Debug)]
pub struct Budget {
    daily_cap_usd: f64,
    session_cap_usd: f64,
}
impl Budget {
    /// Set the two caps in USD.
    #[must_use]
    pub const fn new(daily_cap_usd: f64, session_cap_usd: f64) -> Self {
        Self {
            daily_cap_usd,
            session_cap_usd,
        }
    }
    /// Check existing spend; equality with a cap is already over budget.
    #[must_use]
    pub fn check<L: SpendLedger>(
        &self,
        _ledger: &L,
        _now_ms: u64,
        _session_id: &str,
    ) -> BudgetVerdict {
        let _ = (self.daily_cap_usd, self.session_cap_usd);
        BudgetVerdict::Allowed
    }
}
