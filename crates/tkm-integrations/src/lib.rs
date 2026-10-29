#![forbid(unsafe_code)]
#![deny(missing_docs)]
//! Optional integration points (SPEC 9).
//!
//! The daemon takes these traits by constructor injection. The default implementations do
//! nothing, so tokenimum works alone; epic E9 supplies the otherskill and superorchestrator
//! adapters.

pub mod detect;
pub mod otherskill;
mod process;
pub mod runtime;
pub mod superorchestrator;

use std::{collections::BTreeMap, path::Path};
use tkm_core::model::RepoKey;
use tkm_proto::Host;

/// One skill the otherskill adapter proposes for a prompt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillCandidate {
    /// Stable identifier, used for the relevance question name.
    pub id: String,
    /// One-line text injected when the skill is relevant.
    pub text: String,
}

/// Source of skill candidates for the inject layer (SPEC 7.2, 9.4).
///
/// Called on the prompt path: implementations must answer from memory or a local cache and
/// return quickly.
pub trait SkillSource: Send + Sync {
    /// Returns skill candidates for this prompt; an empty list means none.
    fn candidates(&self, repo: &RepoKey, host: Host, prompt: &str) -> Vec<SkillCandidate>;
}

/// Default [`SkillSource`]: no candidates.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoSkills;

impl SkillSource for NoSkills {
    fn candidates(&self, _repo: &RepoKey, _host: Host, _prompt: &str) -> Vec<SkillCandidate> {
        Vec::new()
    }
}

/// A stuck loop that survived the redirect budget (SPEC 7.4 step 6).
#[derive(Debug, Clone, PartialEq)]
pub struct EscalationEvent {
    /// Harness of the session.
    pub host: Host,
    /// Host-native session identifier.
    pub session_id: String,
    /// Repository identity.
    pub repo: RepoKey,
    /// Tool calls in the stuck turn so far.
    pub turn_calls: u32,
    /// Delegated run identifier.
    pub run_id: Option<String>,
    /// Expected turn envelope.
    pub envelope: u32,
    /// Stuck probability.
    pub stuck: f64,
    /// Progress probability.
    pub progress: f64,
    /// Previous redirect.
    pub redirect_given: Option<String>,
    /// Suggested orchestrator action.
    pub suggest: Suggest,
    /// Probabilities for suggested actions.
    pub suggest_probs: BTreeMap<String, f64>,
}

/// Suggested action; the orchestrator makes the final decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Suggest {
    /// Continue the run.
    Continue,
    /// Redirect the run.
    #[default]
    Redirect,
    /// Cancel the run.
    Cancel,
    /// Reroute the run.
    Reroute,
}

impl Default for EscalationEvent {
    fn default() -> Self {
        Self {
            host: Host::Claude,
            session_id: String::new(),
            repo: RepoKey::default(),
            turn_calls: 0,
            run_id: None,
            envelope: 0,
            stuck: 0.0,
            progress: 0.0,
            redirect_given: None,
            suggest: Suggest::default(),
            suggest_probs: BTreeMap::new(),
        }
    }
}

/// Sink for escalations of delegated runs (SPEC 9.3).
///
/// Called from a background worker, never from the hook path.
pub trait Escalator: Send + Sync {
    /// Registers the hook environment for a session, if applicable.
    fn observe_session(
        &self,
        _host: Host,
        _session_id: &str,
        _cwd: &Path,
        _env: &BTreeMap<String, String>,
    ) {
    }
    /// Returns a cached delegated run identifier, if available.
    fn delegated_run(&self, _cwd: &Path, _env: &BTreeMap<String, String>) -> Option<String> {
        None
    }
    /// Returns the run identifier recorded for a session.
    fn run_id(&self, _host: Host, _session_id: &str) -> Option<String> {
        None
    }
    /// Whether the orchestrator adapter is currently enabled.
    fn superorchestrator_enabled(&self) -> bool {
        false
    }
    /// Returns true when this adapter owns the delegated run of the session, so the daemon
    /// uses the "delegated with adapter" escalation mode.
    fn handles(&self, host: Host, session_id: &str) -> bool;

    /// Delivers the escalation. Failures are the adapter's to log.
    fn escalate(&self, event: &EscalationEvent);
}

/// Default [`Escalator`]: handles nothing.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoEscalator;

impl Escalator for NoEscalator {
    fn handles(&self, _host: Host, _session_id: &str) -> bool {
        false
    }

    fn escalate(&self, _event: &EscalationEvent) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_do_nothing() {
        assert_eq!(
            NoSkills
                .candidates(&RepoKey::default(), Host::Claude, "p")
                .len(),
            0
        );
        assert!(!NoEscalator.handles(Host::Claude, "s"));
    }
}
