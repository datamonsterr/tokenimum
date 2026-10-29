//! E4.3 live layer: judge jobs, verdict delivery and escalation (SPEC 7.4).
//!
//! The hot path only enqueues a judge job. This module runs it off the request path, stores
//! the verdict inside the turn's [`tkm_core::loopguard::TurnGuard`], and the guard hands it back
//! as a redirect on a later `tool_done`. A verdict for a turn that already ended is dropped.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;

use tkm_core::loopguard::{
    JudgeRequest, JudgeScores, JudgeThresholds, JudgeVerdict, RedirectKind, combine_verdict,
};
use tkm_core::model::{Call, RepoKey};
use tkm_decide::api::{Answer, NoulCriteria, Question};
use tkm_decide::cache::Purpose;
use tkm_decide::calibrate::CalibrationKey;
use tkm_decide::client::{DecideRequest, Decision, NoDecisionReason, Outcome};
use tkm_decide::digest::{DigestInput, build_digest};
use tkm_decide::redact::{RedactContext, Redactor};
use tkm_integrations::EscalationEvent;
use tkm_proto::Host;
use tkm_proto::monitor::{DecisionNotice, MonitorMessage};
use tkm_store::NewDecision;

use crate::daemon::{Daemon, lock};
use crate::persist::DecisionInsert;
use crate::settings::{LoopMode, Settings};
use crate::state::Tag;

/// A queued judge request: a snapshot of the turn, owned by the worker.
pub(crate) struct JudgeJob {
    pub tag: Tag,
    pub repo: RepoKey,
    pub key: tkm_store::SessionKey,
    pub seq: u64,
    pub request: JudgeRequest,
    pub prompt: String,
    pub calls: Vec<Call>,
    pub repo_root: PathBuf,
    pub settings: Arc<Settings>,
}

/// A queued escalation for a delegated run.
pub(crate) struct EscalationJob {
    pub host: Host,
    pub session_id: String,
    pub repo: RepoKey,
    pub turn_calls: u32,
    pub with_adapter: bool,
    pub command: Option<String>,
}

const REDIRECT_GUIDANCE: [(&str, &str); 7] = [
    (
        "reread_error",
        "re-read the error and the failing expectation",
    ),
    (
        "check_env",
        "check imports, installed packages and working directory",
    ),
    ("smaller_step", "isolate one hypothesis with a smaller step"),
    (
        "revert_last_edit",
        "revert the last edit and compare the failure",
    ),
    (
        "different_test",
        "run a different test that narrows the cause",
    ),
    (
        "read_more_code",
        "read the surrounding code and its callers",
    ),
    ("ask_user", "ask the user for the missing context"),
];

/// The three loop-guard questions of SPEC 7.4 step 3.
pub fn judge_questions() -> BTreeMap<String, Question> {
    let mut questions = BTreeMap::new();
    questions.insert(
        "stuck".to_owned(),
        Question::Noul {
            instructions: "Is the agent stuck in a loop on this task?".into(),
            criteria: Some(NoulCriteria {
                when_true: "repeats the same failing action or edit without a new approach".into(),
                when_false: "makes progress or tries new approaches".into(),
            }),
        },
    );
    questions.insert(
        "progress".to_owned(),
        Question::Noul {
            instructions: "Did the latest calls change the situation?".into(),
            criteria: Some(NoulCriteria {
                when_true: "the latest calls changed the failure or narrowed the cause".into(),
                when_false: "no change in outcome".into(),
            }),
        },
    );
    questions.insert(
        "redirect".to_owned(),
        Question::Choice {
            instructions: "Which single redirect would help the agent most?".into(),
            criteria: REDIRECT_GUIDANCE
                .iter()
                .map(|(name, text)| ((*name).to_owned(), (*text).to_owned()))
                .collect(),
        },
    );
    questions
}

fn noul(decision: &Decision, name: &str) -> Option<f64> {
    match decision.answers.get(name)? {
        Answer::Noul { noul } => Some(*noul),
        _ => None,
    }
}

fn redirect_kind(decision: &Decision) -> RedirectKind {
    match decision.answers.get("redirect") {
        Some(Answer::Choice { choice, .. }) => choice
            .parse()
            .unwrap_or_else(|_| RedirectKind::rule_default()),
        _ => RedirectKind::rule_default(),
    }
}

impl Daemon {
    /// Runs one judge job to completion (called from the effect worker).
    pub(crate) async fn run_judge(self: &Arc<Self>, job: JudgeJob) {
        let home = self
            .env
            .iter()
            .rev()
            .find(|(k, _)| k == "HOME")
            .map(|(_, v)| PathBuf::from(v))
            .unwrap_or_default();
        let redactor = Redactor::new(RedactContext {
            repo_root: job.repo_root.clone(),
            home,
            env: self.env.clone(),
        });
        let state = build_digest(
            DigestInput {
                prompt: &job.prompt,
                calls: &job.calls,
                tests: None,
            },
            &redactor,
        );
        let questions = judge_questions();
        let request = DecideRequest {
            purpose: Purpose::LoopGuard,
            session_id: format!("{}:{}", job.tag.0.as_str(), job.tag.1),
            state: state.clone(),
            questions: questions.clone(),
        };
        let outcome = self.decider.decide(request).await;
        self.apply_outcome(&job, &questions, &state, outcome);
    }

    fn apply_outcome(
        &self,
        job: &JudgeJob,
        questions: &BTreeMap<String, Question>,
        digest: &str,
        outcome: Outcome,
    ) {
        match outcome {
            Outcome::Decision(decision) => self.apply_decision(job, questions, digest, &decision),
            Outcome::NoDecision(reason) => {
                self.release_judge(job);
                if reason != NoDecisionReason::Disabled {
                    self.log_error(&format!(
                        "{} loop_guard: no decision ({reason:?})",
                        job.tag.0.as_str()
                    ));
                }
            }
            Outcome::CodeOnly(_) => self.release_judge(job),
        }
    }

    fn release_judge(&self, job: &JudgeJob) {
        let mut guard = lock(&self.state);
        if let Some(session) = guard.sessions.get_mut(&job.tag)
            && let Some(turn) = session.turn.as_mut().filter(|t| t.seq == job.seq)
        {
            turn.judging = false;
            if let Some(turn_guard) = turn.guard.as_mut() {
                turn_guard.on_judge_unavailable(job.request);
            }
        }
    }

    fn apply_decision(
        &self,
        job: &JudgeJob,
        questions: &BTreeMap<String, Question>,
        digest: &str,
        decision: &Decision,
    ) {
        let key = CalibrationKey {
            model_version: decision.model_version.clone(),
            purpose: Purpose::LoopGuard.as_str().to_owned(),
        };
        let calibrated =
            tkm_decide::calibrate::CalibrationStore::get(self.calibration.as_ref(), &key);
        let acting = job.settings.loop_mode == LoopMode::Redirect && calibrated.is_some();
        let thresholds = calibrated.unwrap_or_default();

        let evidence = Arc::new(std::sync::atomic::AtomicI64::new(0));
        lock(&self.outbox).decisions.push_back(DecisionInsert {
            evidence_id: evidence.clone(),
            created_at: tkm_core::model::Timestamp(
                i64::try_from(self.clock.now_ms()).unwrap_or(i64::MAX),
            ),
            repo: Some(job.repo.clone()),
            key: Some(job.key.clone()),
            digest: Some(digest.to_owned()),
            decision: NewDecision {
                model: decision.model_version.clone(),
                purpose: key.purpose.clone(),
                questions: serde_json::to_value(questions).unwrap_or_default(),
                answers: serde_json::to_value(&decision.answers).unwrap_or_default(),
                cost_usd: if decision.cached {
                    0.0
                } else {
                    decision.usage.cost.unwrap_or(0.0)
                },
                latency_ms: u32::try_from(decision.latency_ms).unwrap_or(u32::MAX),
                shadow: !acting,
            },
        });
        self.publish(|| {
            MonitorMessage::Decision(DecisionNotice {
                purpose: key.purpose.clone(),
                model: decision.model_version.clone(),
                latency_ms: decision.latency_ms,
                cost_usd: if decision.cached {
                    0.0
                } else {
                    decision.usage.cost.unwrap_or(0.0)
                },
                shadow: !acting,
                answers: serde_json::to_value(&decision.answers).unwrap_or_default(),
            })
        });
        self.publish(|| MonitorMessage::Spend(self.spend_update(&job.settings)));

        let (Some(stuck), Some(progress)) = (noul(decision, "stuck"), noul(decision, "progress"))
        else {
            self.release_judge(job);
            self.log_error("loop_guard: decision lacks the stuck or progress answer");
            return;
        };
        let scores = JudgeScores { stuck, progress };
        let core_thresholds = JudgeThresholds {
            t_stuck: thresholds.t_stuck,
            t_progress: thresholds.t_progress,
            t_low: thresholds.t_low,
        };
        let kind = redirect_kind(decision);

        let mut guard = lock(&self.state);
        let Some(session) = guard.sessions.get_mut(&job.tag) else {
            return;
        };
        let mut shadow_stuck = false;
        if let Some(turn) = session.turn.as_mut().filter(|t| t.seq == job.seq) {
            session.loop_evidence = Some(evidence);
            turn.judging = false;
            if let Some(turn_guard) = turn.guard.as_mut() {
                if acting {
                    turn_guard.on_judge_verdict(job.request, scores, core_thresholds, kind);
                } else {
                    turn_guard.on_judge_unavailable(job.request);
                    shadow_stuck = combine_verdict(scores, core_thresholds) == JudgeVerdict::Stuck;
                }
            }
        }
        // else: the turn ended (or a new one began) while the decision ran: drop the verdict.
        if shadow_stuck {
            session.shadow = true;
            let text = kind.render(&tkm_core::loopguard::RedirectParams {
                calls: job.request.call_index,
                failing_command: "",
                error_line: "",
            });
            self.record_loop_event(session, "loop.shadow", kind.as_str(), &text, true);
        }
    }

    /// Delivers an escalation of a delegated run (adapter call or user command).
    pub(crate) async fn run_escalation(self: &Arc<Self>, job: EscalationJob) {
        let me = self.clone();
        let joined = tokio::task::spawn_blocking(move || {
            if job.with_adapter {
                me.escalator.escalate(&EscalationEvent {
                    host: job.host,
                    session_id: job.session_id.clone(),
                    repo: job.repo.clone(),
                    turn_calls: job.turn_calls,
                });
                return Ok(());
            }
            let Some(command) = job.command.as_deref() else {
                return Ok(());
            };
            std::process::Command::new("sh")
                .arg("-c")
                .arg(command)
                .env("TKM_HOST", job.host.as_str())
                .env("TKM_SESSION", &job.session_id)
                .env("TKM_REPO", &job.repo.0)
                .env("TKM_TURN_CALLS", job.turn_calls.to_string())
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status()
                .map(|_| ())
        })
        .await;
        match joined {
            Ok(Ok(())) => {}
            Ok(Err(error)) => self.log_error(&format!("loop.escalate_command: {error}")),
            Err(error) => self.log_error(&format!("escalation task: {error}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn judge_questions_follow_the_spec_table() {
        let questions = judge_questions();
        assert_eq!(questions.len(), 3);
        assert!(matches!(questions["stuck"], Question::Noul { .. }));
        assert!(matches!(questions["progress"], Question::Noul { .. }));
        let Question::Choice { criteria, .. } = &questions["redirect"] else {
            panic!("redirect must be a choice");
        };
        assert_eq!(criteria.len(), 7);
        for name in criteria.keys() {
            assert!(name.parse::<RedirectKind>().is_ok(), "{name}");
        }
    }
}
