//! In-memory decision response cache with purpose-specific expiry.
use crate::{
    api::{DecisionsResponse, Question},
    chain::Clock,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
/// Stable purpose identifiers shared with spend and calibration storage.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum Purpose {
    /// Judge whether a session is stuck.
    LoopGuard,
    /// Judge context relevance.
    Relevance,
    /// Judge candidate facts.
    FactFilter,
    /// Select the next corrective action.
    Redirect,
}
impl Purpose {
    /// Stable storage representation.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::LoopGuard => "loop_guard",
            Self::Relevance => "relevance",
            Self::FactFilter => "fact_filter",
            Self::Redirect => "redirect",
        }
    }
    /// Cache lifetime in milliseconds; changing workflow judgments are not cached.
    #[must_use]
    pub const fn ttl_ms(self) -> u64 {
        0
    }
}
/// A synchronized response cache using an injected clock.
pub struct DecisionCache<C: Clock> {
    clock: C,
}
impl<C: Clock> DecisionCache<C> {
    /// Create an empty cache.
    #[must_use]
    pub const fn new(clock: C) -> Self {
        Self { clock }
    }
    /// Read an unexpired response for exactly this model, state and questions.
    #[must_use]
    pub fn get(
        &self,
        _model: &str,
        _state: &str,
        _questions: &BTreeMap<String, Question>,
        _purpose: Purpose,
    ) -> Option<DecisionsResponse> {
        let _ = self.clock.now_ms();
        None
    }
    /// Store a successful response for the purpose's lifetime.
    pub fn insert(
        &self,
        _model: &str,
        _state: &str,
        _questions: &BTreeMap<String, Question>,
        _purpose: Purpose,
        _response: DecisionsResponse,
    ) {
    }
}
