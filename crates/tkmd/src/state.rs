//! In-memory per-session and per-turn state, keyed by (host, session id).

use std::collections::{BTreeSet, HashMap};
use std::path::PathBuf;
use std::sync::Arc;

use tkm_core::loopguard::{EnvelopeTracker, LoopConfig, TurnGuard};
use tkm_core::model::{Call, RepoKey, StopReason, Timestamp, Turn};
use tkm_proto::Host;
use tkm_store::SessionKey;

use crate::settings::Settings;

/// Key of a live session.
pub type Tag = (Host, String);

/// Most calls kept per turn for the digest and the envelope check.
pub const MAX_TURN_CALLS: usize = 500;

/// A turn in progress.
#[derive(Debug)]
pub struct TurnState {
    /// Daemon-wide sequence number; a judge verdict carries it so a verdict for an ended turn is
    /// dropped.
    pub seq: u64,
    /// Position within the session (the `turns.turn_idx` row).
    pub idx: i64,
    /// Prompt text (first 500 characters are used in digests).
    pub prompt: String,
    /// Loop guard, absent when `loop.mode = off`.
    pub guard: Option<TurnGuard>,
    /// Calls so far, capped at [`MAX_TURN_CALLS`].
    pub calls: Vec<Call>,
    /// Total calls, including those beyond the cap.
    pub total_calls: u32,
    /// Redirects delivered.
    pub redirects: u32,
    /// Last delivered redirect kind.
    pub redirect_kind: Option<String>,
    /// Last judge scores for this turn.
    pub judge_scores: Option<(f64, f64)>,
    /// Whether this turn escalated.
    pub escalated: bool,
    /// Whether a judge request is in flight.
    pub judging: bool,
    /// Decision IDs and their call positions, used to label completed turns.
    pub decision_evidence: Vec<(Arc<std::sync::atomic::AtomicI64>, u32)>,
    /// Redirect waiting for an injectable event in this turn.
    pub pending_redirect: Option<String>,
}

/// A live session.
#[derive(Debug)]
pub struct SessionState {
    /// Unique incarnation of this in-memory session.
    pub generation: u64,
    /// Database natural key.
    pub key: SessionKey,
    /// Harness.
    pub host: Host,
    /// Host-native session id.
    pub session_id: String,
    /// Repository identity (empty until resolved).
    pub repo: RepoKey,
    /// Human-readable repository name.
    pub repo_label: String,
    /// Repository (or start directory) root.
    pub root: Option<PathBuf>,
    /// Working directory.
    pub cwd: PathBuf,
    /// Session log path, when the hook payload names one.
    pub log_path: Option<PathBuf>,
    /// Agent home directory.
    pub agent_home: PathBuf,
    /// Settings resolved at session start.
    pub settings: Arc<Settings>,
    /// Start time.
    pub started: Timestamp,
    /// Last hook event or log write, Unix milliseconds.
    pub last_activity_ms: u64,
    /// Next `turn_idx`.
    pub next_turn_idx: i64,
    /// Current turn, if any.
    pub turn: Option<TurnState>,
    /// Block waiting for an injectable event.
    pub pending_inject: Option<String>,
    /// Candidate ids already injected in this session.
    pub injected: BTreeSet<String>,
    /// True for runs the person did not start interactively (`TKM_DELEGATED` marker).
    pub delegated: bool,
    /// True once repository identity, settings and the snapshot were loaded.
    pub resolved: bool,
    /// True when any loop action was shadowed.
    pub shadow: bool,
    /// Learned envelope for this session's repository (cached; see [`EnvelopeEntry`]).
    pub envelope: Option<f64>,
    /// Loop guard state for the monitor.
    pub loop_state: &'static str,
    /// Exact model decision behind the current turn, resolved to its stored ID by the flusher.
    pub loop_evidence: Option<Arc<std::sync::atomic::AtomicI64>>,
    /// Native hook names already queued for doctor/status coverage in this session.
    pub observed_hooks: BTreeSet<String>,
}

/// Envelope tracker with its cached value, so `tool_done` never sorts samples.
#[derive(Debug, Default)]
pub struct EnvelopeEntry {
    /// Good-turn lengths.
    pub tracker: EnvelopeTracker,
    /// Cached `value()` for the loop settings in force.
    pub value: Option<f64>,
}

impl EnvelopeEntry {
    /// Rebuilds the entry from stored samples.
    pub fn from_samples(samples: &[u32], cfg: &LoopConfig) -> Self {
        let mut tracker = EnvelopeTracker::default();
        for calls in samples {
            let n = usize::try_from(*calls).unwrap_or(0);
            let turn = Turn {
                calls: vec![Call::default(); n],
                stopped_by: StopReason::OwnStop,
                ..Turn::default()
            };
            tracker.record(&turn, false);
        }
        let value = tracker.value(cfg);
        Self { tracker, value }
    }

    /// Records a finished turn; returns true when it was a good turn.
    pub fn record(&mut self, turn: &Turn, escalated: bool, cfg: &LoopConfig) -> bool {
        let good = self.tracker.record(turn, escalated);
        if good {
            self.value = self.tracker.value(cfg);
        }
        good
    }
}

/// All mutable daemon state behind one lock.
#[derive(Debug, Default)]
pub struct StateMap {
    /// Source of session incarnation identifiers.
    pub session_generation: u64,
    /// Live sessions.
    pub sessions: HashMap<Tag, SessionState>,
    /// Envelopes per (repository, host).
    pub envelopes: HashMap<(RepoKey, Host), EnvelopeEntry>,
    /// Source of [`TurnState::seq`].
    pub turn_seq: u64,
    /// Next `turn_idx` of sessions that left memory, so a resumed session continues the count
    /// even before its rows were flushed.
    pub turn_bases: HashMap<Tag, i64>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn envelope_entry_value_follows_samples() {
        let cfg = LoopConfig::default();
        let entry = EnvelopeEntry::from_samples(&[5, 5, 5, 5, 5], &cfg);
        assert_eq!(entry.value, Some(6.25));
        let short = EnvelopeEntry::from_samples(&[5, 5], &cfg);
        assert_eq!(short.value, None, "fewer than envelope_min_samples");
    }
}
