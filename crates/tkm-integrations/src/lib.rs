#![forbid(unsafe_code)]
#![deny(missing_docs)]
//! Optional integration points (SPEC 9).
//!
//! The daemon takes these traits by constructor injection. The default implementations do
//! nothing, so tokenimum works alone; epic E9 supplies the otherskill and superorchestrator
//! adapters.

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
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EscalationEvent {
    /// Harness of the session.
    pub host: Host,
    /// Host-native session identifier.
    pub session_id: String,
    /// Repository identity.
    pub repo: RepoKey,
    /// Tool calls in the stuck turn so far.
    pub turn_calls: u32,
}

/// Sink for escalations of delegated runs (SPEC 9.3).
///
/// Called from a background worker, never from the hook path.
pub trait Escalator: Send + Sync {
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
        assert!(
            NoSkills
                .candidates(&RepoKey::default(), Host::Claude, "p")
                .is_empty()
        );
        assert!(!NoEscalator.handles(Host::Claude, "s"));
    }
}
