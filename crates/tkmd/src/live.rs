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
use tkm_integrations::{EscalationEvent, Suggest};
use tkm_proto::Host;
use tkm_proto::monitor::{DecisionNotice, MonitorMessage};
use tkm_store::NewDecision;

use crate::daemon::{Daemon, lock};
use crate::settings::{LoopMode, Settings};
use crate::state::Tag;

/// A queued judge request: a snapshot of the turn, owned by the worker.
pub(crate) struct JudgeJob {
    pub tag: Tag,
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
    pub envelope: u32,
    pub redirect_given: Option<String>,
    pub scores: Option<(f64, f64)>,
    pub prompt: String,
    pub calls: Vec<Call>,
    pub repo_root: PathBuf,
    pub with_adapter: bool,
    pub decide_enabled: bool,
    pub delegated_run: Option<tkm_integrations::superorchestrator::DelegatedRun>,
    pub command: Option<String>,
    pub secrets: BTreeMap<String, String>,
}

fn escalation_redactor(
    repo_root: PathBuf,
    home: PathBuf,
    daemon_env: &[(String, String)],
    session_env: &BTreeMap<String, String>,
) -> Redactor {
    let mut env = daemon_env.to_vec();
    env.extend(session_env.iter().map(|(k, v)| (k.clone(), v.clone())));
    Redactor::new(RedactContext {
        repo_root,
        home,
        env,
    })
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
            state,
            questions: questions.clone(),
        };
        let outcome = self.decider.decide(request).await;
        self.apply_outcome(&job, &questions, outcome);
    }

    fn apply_outcome(
        &self,
        job: &JudgeJob,
        questions: &BTreeMap<String, Question>,
        outcome: Outcome,
    ) {
        match outcome {
            Outcome::Decision(decision) => self.apply_decision(job, questions, &decision),
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

        lock(&self.outbox).decisions.push_back(NewDecision {
            model: decision.model_version.clone(),
            purpose: key.purpose.clone(),
            questions: serde_json::to_value(questions).unwrap_or_default(),
            answers: serde_json::to_value(&decision.answers).unwrap_or_default(),
            cost_usd: decision.usage.cost.unwrap_or(0.0),
            latency_ms: u32::try_from(decision.latency_ms).unwrap_or(u32::MAX),
            shadow: !acting,
        });
        self.publish(|| {
            MonitorMessage::Decision(DecisionNotice {
                purpose: key.purpose.clone(),
                model: decision.model_version.clone(),
                latency_ms: decision.latency_ms,
                cost_usd: decision.usage.cost.unwrap_or(0.0),
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
            turn.judging = false;
            turn.judge_scores = Some((stuck, progress));
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
        let use_adapter = job.with_adapter
            && (job.delegated_run.is_some() || self.escalator.handles(job.host, &job.session_id));
        let mut suggest = Suggest::Redirect;
        let mut suggest_probs = BTreeMap::new();
        if use_adapter && job.decide_enabled {
            let home = self
                .env
                .iter()
                .rev()
                .find(|(key, _)| key == "HOME")
                .map_or_else(PathBuf::new, |(_, value)| PathBuf::from(value));
            let redactor =
                escalation_redactor(job.repo_root.clone(), home, &self.env, &job.secrets);
            let digest = build_digest(
                DigestInput {
                    prompt: &job.prompt,
                    calls: &job.calls,
                    tests: None,
                },
                &redactor,
            );
            let question = Question::Choice {
                instructions: "Which action should the orchestrator take?".into(),
                criteria: ["continue", "redirect", "cancel", "reroute"]
                    .into_iter()
                    .map(|value| (value.to_owned(), value.to_owned()))
                    .collect(),
            };
            let request = DecideRequest {
                purpose: Purpose::Redirect,
                session_id: format!("{}:{}", job.host.as_str(), job.session_id),
                state: format!("{digest}\nrouting level: unknown"),
                questions: BTreeMap::from([("suggest".to_owned(), question)]),
            };
            if let Ok(Outcome::Decision(decision)) = tokio::time::timeout(
                std::time::Duration::from_secs(3),
                self.decider.decide(request),
            )
            .await
            {
                lock(&self.outbox).decisions.push_back(NewDecision {
                    model: decision.model_version.clone(),
                    purpose: Purpose::Redirect.as_str().to_owned(),
                    questions: serde_json::Value::Null,
                    answers: serde_json::to_value(&decision.answers).unwrap_or_default(),
                    cost_usd: decision.usage.cost.unwrap_or(0.0),
                    latency_ms: u32::try_from(decision.latency_ms).unwrap_or(u32::MAX),
                    shadow: false,
                });
                if let Some(Answer::Choice {
                    choice,
                    probabilities,
                    ..
                }) = decision.answers.get("suggest")
                {
                    suggest = match choice.as_str() {
                        "continue" => Suggest::Continue,
                        "cancel" => Suggest::Cancel,
                        "reroute" => Suggest::Reroute,
                        _ => Suggest::Redirect,
                    };
                    suggest_probs = probabilities.clone().unwrap_or_default();
                }
            }
        }
        let me = self.clone();
        let joined = tokio::task::spawn_blocking(move || {
            if use_adapter
                && me.escalator.try_escalate_with_run(
                    &EscalationEvent {
                        host: job.host,
                        session_id: job.session_id.clone(),
                        repo: job.repo.clone(),
                        turn_calls: job.turn_calls,
                        run_id: job
                            .delegated_run
                            .as_ref()
                            .map(|run| run.run_id.clone())
                            .or_else(|| me.escalator.run_id(job.host, &job.session_id)),
                        envelope: job.envelope,
                        stuck: job.scores.map_or(0.0, |scores| scores.0),
                        progress: job.scores.map_or(0.0, |scores| scores.1),
                        redirect_given: job.redirect_given.clone(),
                        suggest,
                        suggest_probs,
                    },
                    job.delegated_run.as_ref(),
                )
            {
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
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::pedantic)]
    use super::*;

    #[tokio::test]
    async fn queued_escalation_survives_fold_and_falls_back_only_on_failure() {
        use std::{fs, os::unix::fs::PermissionsExt};
        use tkm_integrations::{
            Escalator,
            detect::{Mode, SystemClock},
            runtime::Integrations,
        };
        use tkm_proto::{EventKind, PROTOCOL_VERSION, Request};
        for succeeds in [false, true] {
            let dir = tempfile::tempdir().unwrap();
            let binary = dir.path().join("superorchestrator");
            let log = dir.path().join("rpc");
            let marker = dir.path().join("marker");
            let mcp = if succeeds {
                format!(
                    r#"while IFS= read -r line; do printf '%s\n' "$line" >> '{}'; case "$line" in *'"id":1'*) echo '{{"jsonrpc":"2.0","id":1,"result":{{"protocolVersion":"2025-06-18"}}}}';; *'"id":2'*) echo '{{"jsonrpc":"2.0","id":2,"result":{{"content":[{{"type":"text","text":"ok"}}]}}}}';; esac; done"#,
                    log.display()
                )
            } else {
                "exit 1".to_owned()
            };
            fs::write(&binary, format!("#!/bin/sh\nif [ \"$1\" = --version ]; then exit 0; fi\nif [ \"$1\" = runs ]; then printf '[]'; exit 0; fi\n{mcp}\n")).unwrap();
            fs::set_permissions(&binary, fs::Permissions::from_mode(0o755)).unwrap();
            let adapter = Arc::new(Integrations::new(
                Mode::On,
                Mode::Off,
                dir.path().as_os_str().to_owned(),
                dir.path().to_path_buf(),
                BTreeMap::new(),
                Arc::new(SystemClock),
            ));
            adapter.detect_now(dir.path());
            let paths = tkm_config::paths::Paths::from_dirs(
                dir.path(),
                Some(&dir.path().join("data")),
                Some(&dir.path().join("config")),
                Some(&dir.path().join("run")),
            );
            let mut options = crate::daemon::DaemonOptions::new(paths);
            options.escalator = adapter.clone();
            let daemon = Daemon::new(options);
            let env = BTreeMap::from([
                ("SUPERORCHESTRATOR_RUN_ID".into(), "run".into()),
                ("SUPERORCHESTRATOR_RUN_TOKEN".into(), "secret".into()),
            ]);
            let request = Request {
                version: PROTOCOL_VERSION,
                host: Host::Claude,
                event: EventKind::SessionStart,
                host_event: Some("SessionStart".into()),
                payload: serde_json::json!({"session_id":"s", "hook_event_name":"SessionStart", "cwd":"/tmp"}),
                env,
                cwd: None,
            };
            daemon.handle(request).await;
            let run = adapter.delegated_run_for(Host::Claude, "s");
            assert!(run.is_some());
            let job = EscalationJob {
                host: Host::Claude,
                session_id: "s".into(),
                repo: RepoKey::default(),
                turn_calls: 1,
                envelope: 0,
                redirect_given: None,
                scores: None,
                prompt: String::new(),
                calls: Vec::new(),
                repo_root: dir.path().to_path_buf(),
                with_adapter: true,
                decide_enabled: true,
                delegated_run: run,
                command: Some(format!("printf fallback > '{}'", marker.display())),
                secrets: BTreeMap::new(),
            };
            assert!(daemon.enqueue(crate::daemon::Effect::Escalate(job)));
            daemon.fold_live_session(&(Host::Claude, "s".into()), true);
            assert!(!adapter.handles(Host::Claude, "s"));
            daemon.start_workers();
            daemon.settle().await;
            assert_eq!(marker.exists(), !succeeds);
            if succeeds {
                assert!(fs::read_to_string(log).unwrap().contains("report_progress"));
            }
        }
    }

    #[test]
    fn escalation_digest_redacts_session_token() {
        let env = BTreeMap::from([(
            "SUPERORCHESTRATOR_RUN_TOKEN".to_owned(),
            "session-token-value".to_owned(),
        )]);
        let redactor = escalation_redactor(PathBuf::new(), PathBuf::new(), &[], &env);
        assert!(
            !redactor
                .redact("output session-token-value")
                .contains("session-token-value")
        );
    }

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

#[cfg(test)]
#[path = "e9_review_tests.rs"]
mod e9_review_tests;
