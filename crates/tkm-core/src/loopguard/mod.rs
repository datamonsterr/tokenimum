//! Pure per-turn loop detection, redirects, and repository envelopes (SPEC 7.4).
//!
//! No action stops or cancels a turn. The synthetic golden trajectory opens the
//! judge gate at call 18 and reaches the strict backstop at call 21 (envelope 8).
//! Calls are numbered from one; segment resets retain whole-turn evidence.
pub mod envelope;
pub mod redirect;
pub mod rules;
pub mod shell_edits;
pub mod state;
pub mod turn;
pub use envelope::{EnvelopeTracker, is_good_turn};
pub use redirect::{RedirectKind, RedirectParams, stop_key};
pub use rules::{
    JudgeScores, JudgeThresholds, JudgeVerdict, LoopConfig, combine_verdict, gate_for_judge,
    is_stuck,
};
pub use state::SpiralState;
pub use turn::{EscalationMode, JudgeRequest, LoopAction, TurnGuard};
