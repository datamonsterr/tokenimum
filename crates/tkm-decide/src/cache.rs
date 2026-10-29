//! In-memory decision response cache with purpose-specific expiry.
use crate::{
    api::{DecisionsResponse, Question},
    chain::Clock,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, HashMap},
    sync::Mutex,
};
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
        match self {
            Self::Relevance => 600_000,
            Self::FactFilter => 86_400_000,
            Self::LoopGuard | Self::Redirect => 0,
        }
    }
}
struct Entry {
    inserted_ms: u64,
    expires_ms: u64,
    response: DecisionsResponse,
}
#[derive(PartialEq, Eq, Hash)]
struct CacheKey {
    model: String,
    state: String,
    questions_json: String,
}
/// A synchronized response cache using an injected clock.
#[allow(clippy::module_name_repetitions)] // Public names follow the decision-client contract.
pub struct DecisionCache<C: Clock> {
    entries: Mutex<HashMap<CacheKey, Entry>>,
    clock: C,
}
impl<C: Clock> DecisionCache<C> {
    /// Create an empty cache.
    #[must_use]
    pub fn new(clock: C) -> Self {
        Self {
            entries: Mutex::new(HashMap::new()),
            clock,
        }
    }
    /// Read an unexpired response for exactly this model, state and questions.
    #[must_use]
    pub fn get(
        &self,
        model: &str,
        state: &str,
        questions: &BTreeMap<String, Question>,
        purpose: Purpose,
    ) -> Option<DecisionsResponse> {
        if purpose.ttl_ms() == 0 {
            return None;
        }
        let key = cache_key(model, state, questions)?;
        let now = self.clock.now_ms();
        let mut entries = self
            .entries
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        entries.retain(|_, entry| now < entry.expires_ms);
        let entry = entries.get(&key)?;
        // A short-lived purpose must not inherit a longer TTL from another purpose.
        if now < entry.inserted_ms || now.saturating_sub(entry.inserted_ms) >= purpose.ttl_ms() {
            return None;
        }
        Some(entry.response.clone())
    }
    /// Store a successful response for the purpose's lifetime.
    pub fn insert(
        &self,
        model: &str,
        state: &str,
        questions: &BTreeMap<String, Question>,
        purpose: Purpose,
        response: DecisionsResponse,
    ) {
        if purpose.ttl_ms() == 0 {
            return;
        }
        let Some(key) = cache_key(model, state, questions) else {
            return;
        };
        let now = self.clock.now_ms();
        let mut entries = self
            .entries
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        entries.retain(|_, entry| now < entry.expires_ms);
        entries.insert(
            key,
            Entry {
                inserted_ms: now,
                expires_ms: now.saturating_add(purpose.ttl_ms()),
                response,
            },
        );
    }
}
fn cache_key(model: &str, state: &str, questions: &BTreeMap<String, Question>) -> Option<CacheKey> {
    let questions_json = serde_json::to_string(questions).ok()?;
    Some(CacheKey {
        model: model.into(),
        state: state.into(),
        questions_json,
    })
}
