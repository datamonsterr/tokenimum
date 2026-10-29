// Portions ported from microsoft/TokenEconomizer (tokeneconomizer/loopguard/turn.py; tokeneconomizer/plugin/controller.py, commit d0675a3),
// Copyright (c) Microsoft Corporation, licensed under the Apache License, Version 2.0.
//! Turn fold, asynchronous verdict correlation, and the redirect/escalation ladder.
use super::{JudgeScores, JudgeThresholds, JudgeVerdict, LoopConfig, RedirectKind, SpiralState};
use super::{
    RedirectParams, combine_verdict, gate_for_judge, is_stuck,
    redirect::{command_evidence, error_evidence},
    verification,
};
use crate::model::Call;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
struct FailureEvidence {
    command: String,
    error: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct ReadyRedirect {
    kind: RedirectKind,
    evidence: FailureEvidence,
}
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
    /// Inject a code-template redirect at this `tool_done`.
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
    /// Whether a code-rule backstop fired during this turn.
    #[serde(default)]
    pub backstop_triggered: bool,
    mode: EscalationMode,
    // Outstanding job identity survives segment resets until its completion/release.
    pending: Option<JudgeRequest>,
    last_judge_call: Option<u32>,
    ready: Option<ReadyRedirect>,
    last_failure: FailureEvidence,
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
            backstop_triggered: false,
            mode,
            pending: None,
            last_judge_call: None,
            ready: None,
            last_failure: FailureEvidence::default(),
        }
    }
    /// Fold one `tool_done` and return advisory actions (SPEC 7.4).
    pub fn on_tool_done(
        &mut self,
        call: &Call,
        cfg: &LoopConfig,
        envelope: Option<f64>,
    ) -> Vec<LoopAction> {
        let errors_before = self.whole.error_outputs;
        self.segment.update(call);
        self.whole.update(call);
        if self.whole.error_outputs > errors_before {
            self.last_failure = FailureEvidence {
                command: command_evidence(&call.args_digest),
                error: error_evidence(
                    verification::error_line(&call.output_digest).unwrap_or_default(),
                ),
            };
        }
        let Some(envelope) = envelope.filter(|n| n.is_finite() && *n > 0.0) else {
            return Vec::new();
        };
        if self.escalated {
            return Vec::new();
        }
        if let Some(ready) = self.ready.take() {
            return vec![self.act(ready.kind, &ready.evidence, cfg)];
        }
        let backstop = envelope * cfg.backstop_mult;
        if cfg.backstop_mult.is_finite()
            && cfg.backstop_mult > 0.0
            && backstop.is_finite()
            && f64::from(self.whole.turns) > backstop
            && is_stuck(&self.segment, cfg)
        {
            self.backstop_triggered = true;
            return vec![self.act(
                RedirectKind::rule_default(),
                &self.last_failure.clone(),
                cfg,
            )];
        }
        if self.pending.is_none()
            && gate_for_judge(&self.segment, cfg)
            && self
                .last_judge_call
                .is_none_or(|last| self.whole.turns.saturating_sub(last) >= 5)
        {
            let request = JudgeRequest {
                segment: self.segment_id,
                call_index: self.whole.turns,
            };
            self.pending = Some(request);
            self.last_judge_call = Some(self.whole.turns);
            return vec![LoopAction::RequestJudge { request }];
        }
        Vec::new()
    }
    /// Release a matching outstanding job; a current-segment Stuck result queues a redirect.
    /// Older-segment results release the job but never affect the current segment.
    pub fn on_judge_verdict(
        &mut self,
        request: JudgeRequest,
        scores: JudgeScores,
        thresholds: JudgeThresholds,
        kind: RedirectKind,
    ) -> JudgeVerdict {
        if self.pending != Some(request) {
            return JudgeVerdict::Wait;
        }
        self.pending = None;
        if request.segment != self.segment_id || self.escalated {
            return JudgeVerdict::Wait;
        }
        let verdict = combine_verdict(scores, thresholds);
        if verdict == JudgeVerdict::Stuck {
            self.ready = Some(ReadyRedirect {
                kind,
                evidence: self.last_failure.clone(),
            });
        }
        verdict
    }
    /// Release a failed or timed-out judge request; all clocks and I/O belong to the caller.
    /// The caller must confirm the job no longer runs before releasing a timeout.
    pub fn on_judge_unavailable(&mut self, request: JudgeRequest) {
        if self.pending == Some(request) {
            self.pending = None;
        }
    }

    fn act(
        &mut self,
        kind: RedirectKind,
        evidence: &FailureEvidence,
        cfg: &LoopConfig,
    ) -> LoopAction {
        self.ready = None;
        if self.redirects_done >= cfg.redirects_per_turn {
            self.escalated = true;
            return LoopAction::Escalate {
                mode: self.mode.clone(),
            };
        }
        let text = kind.render(&RedirectParams {
            calls: self.segment.turns,
            failing_command: &evidence.command,
            error_line: &evidence.error,
        });
        self.redirects_done = self.redirects_done.saturating_add(1);
        self.segment_id = self.segment_id.saturating_add(1);
        self.segment = SpiralState::default();
        self.last_failure = FailureEvidence::default();
        LoopAction::Redirect {
            redirect_kind: kind,
            text,
        }
    }
}
