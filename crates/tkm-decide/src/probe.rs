//! Shared privacy-safe model availability request used by doctor and daemon maintenance.
use crate::{
    api::Question,
    cache::Purpose,
    client::{DecideRequest, Outcome},
};
use std::collections::BTreeMap;

/// One configured free slot's probe outcome.
#[derive(Clone, Debug, PartialEq)]
pub struct FreeProbeResult {
    /// Configured slug; used to restore model-chain health.
    pub model: String,
    /// Validated response or a fail-open result.
    pub outcome: Outcome,
}

/// Build a bounded synthetic request containing no repository or session data.
#[must_use]
pub fn availability_request(session_id: &str) -> DecideRequest {
    DecideRequest {
        purpose: Purpose::Relevance,
        session_id: session_id.into(),
        state: "{\"probe_marker\":\"tokenimum\"}".into(),
        questions: BTreeMap::from([(
            "available".into(),
            Question::Noul {
                instructions: "Is the probe marker equal to tokenimum?".into(),
                criteria: None,
            },
        )]),
    }
}
