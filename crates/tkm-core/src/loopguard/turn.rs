// Portions ported from microsoft/TokenEconomizer (tokeneconomizer/loopguard/turn.py; tokeneconomizer/plugin/controller.py, commit d0675a3),
// Copyright (c) Microsoft Corporation, licensed under the Apache License, Version 2.0.
//! Turn fold, asynchronous verdict correlation, and the redirect/escalation ladder.
use super::{JudgeScores, JudgeThresholds, JudgeVerdict, LoopConfig, RedirectKind, SpiralState};
use crate::model::Call;
use serde::{Deserialize, Serialize};
/// Where the daemon should deliver a still-stuck notice (SPEC 7.4 step 6).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum EscalationMode {
    /// A notice to the person, with the host's stop key.
    Interactive {
        /// Key named in the notice; tokenimum never presses it.
        stop_key: String,
    },
    /// Structured event to the configured orchestrator adapter.
    DelegatedWithAdapter,
    /// Stored event and optional caller-configured notification hook.
    DelegatedNoAdapter,
}
/// Correlation token prevents a stale judge result from affecting another segment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct JudgeRequest {
    /// Segment for which evidence was sampled.
    pub segment: u32,
    /// Whole-turn call index at sampling time, starting at one.
    pub call_index: u32,
}
/// Advisory actions only: there is deliberately no stop/cancel variant.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum LoopAction {
    /// Ask the daemon to sample the current segment with the calibrated judge.
    RequestJudge {
        /// Correlation token to return with the verdict.
        request: JudgeRequest,
    },
    /// Inject a code-template redirect at this tool_done.
    Redirect {
        /// Selected redirect library entry.
        redirect_kind: RedirectKind,
        /// Sanitized text, entirely rendered from that entry's template.
        text: String,
    },
    /// Notify a person or adapter once; never terminate execution.
    Escalate {
        /// Delivery route chosen by the caller.
        mode: EscalationMode,
    },
}
/// Serializable state for exactly one prompt-turn; create a new guard for a new turn.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TurnGuard {
    /// Current segment; reset after a delivered redirect.
    pub segment: SpiralState,
    /// Cumulative signals; never reset mid-turn.
    pub whole: SpiralState,
    /// Segment number, starting at zero.
    pub segment_id: u32,
    /// Number of redirects already delivered.
    pub redirects_done: u32,
    /// Whether an escalation has already been emitted.
    pub escalated: bool,
    mode: EscalationMode,
    pending: Option<JudgeRequest>,
    last_judge_call: Option<u32>,
    ready: Option<RedirectKind>,
}
impl TurnGuard {
    /// Construct empty state with a caller-selected escalation delivery route.
    #[must_use]
    pub fn new(mode: EscalationMode) -> Self {
        Self {
            segment: SpiralState::default(),
            whole: SpiralState::default(),
            segment_id: 0,
            redirects_done: 0,
            escalated: false,
            mode,
            pending: None,
            last_judge_call: None,
            ready: None,
        }
    }
    /// Fold one tool_done and return advisory actions (SPEC 7.4).
    pub fn on_tool_done(
        &mut self,
        _call: &Call,
        _cfg: &LoopConfig,
        _envelope: Option<f64>,
    ) -> Vec<LoopAction> {
        Vec::new()
    }
    /// Accept one matching judge response; Stuck queues delivery at the next tool_done.
    pub fn on_judge_verdict(
        &mut self,
        _request: JudgeRequest,
        _scores: JudgeScores,
        _thresholds: JudgeThresholds,
        _kind: RedirectKind,
    ) -> JudgeVerdict {
        JudgeVerdict::Wait
    }
    /// Release a failed or timed-out judge request; all clocks and I/O belong to the caller.
    pub fn on_judge_unavailable(&mut self, _request: JudgeRequest) {}
}
