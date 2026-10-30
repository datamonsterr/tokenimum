// TE oracle rules derive from microsoft/TokenEconomizer commit d0675a3,
// Copyright (c) Microsoft Corporation, Apache License 2.0; see NOTICE.
//! SPEC 16 same-log parity, including positive fact formation and a waking loop gate.
//! Oracle JSON is generated independently by scripts/te_parity_oracle.py, never blessed
//! from tokenimum output. Every difference is asserted here and explained in docs/parity.md.
#![forbid(unsafe_code)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::pedantic)]

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use tkm_core::facts::{
    self, DecisionScores, Decisions, FactsConfig, FactsState, SessionEvidence, lifecycle::Status,
    observe,
};
use tkm_core::loopguard::{EscalationMode, LoopAction, LoopConfig, TurnGuard};
use tkm_core::model::{RepoKey, Session};
use tkm_hosts::{HostAdapter, SessionSource, claude::ClaudeAdapter, codex::CodexAdapter};
use tkmd::fold::replay_fold;

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

#[derive(Deserialize)]
struct Oracle {
    te_commit: String,
    cases: Vec<Case>,
}
#[derive(Deserialize)]
struct Case {
    source: String,
    calls_per_turn: Vec<usize>,
    candidate_ids: Vec<String>,
    te_active_after_sessions: Vec<Vec<String>>,
    code_only_active_after_sessions: Vec<Vec<String>>,
    loops: Vec<LoopOracle>,
}
#[derive(Debug, Deserialize, PartialEq, Eq)]
struct LoopOracle {
    judge_calls: Vec<usize>,
    backstop_calls: Vec<usize>,
}

fn active(state: &FactsState) -> Vec<String> {
    state
        .facts
        .iter()
        .filter(|(_, fact)| fact.life.status == Status::Active)
        .map(|(id, _)| id.clone())
        .collect()
}

fn compare_facts(session: &Session, case: &Case) {
    let candidates = facts::extract::extract(&observe::observe(session));
    let mut ids: Vec<_> = candidates
        .iter()
        .map(|candidate| candidate.id.clone())
        .collect();
    ids.sort();
    assert_eq!(ids, case.candidate_ids, "{}: formation", case.source);
    let mut te_state = FactsState::default();
    let mut code_state = FactsState::default();
    assert_eq!(case.te_active_after_sessions.len(), 4);
    assert_eq!(case.code_only_active_after_sessions.len(), 4);
    for index in 0..4 {
        let mut session = session.clone();
        // Four distinct sessions exercising exactly these logs. This is not an idempotent replay.
        session.session_id = format!("parity-{}-{index}", case.source);
        let decisions = Decisions {
            t_low: 0.3,
            t_high: 0.8,
            scores: ids
                .iter()
                .map(|id| {
                    (
                        id.clone(),
                        DecisionScores {
                            stable: Some(1.0),
                            safe: Some(1.0),
                            contradicts: None,
                        },
                    )
                })
                .collect(),
        };
        te_state = facts::update(
            te_state,
            &SessionEvidence {
                session: session.clone(),
                injected_ids: BTreeSet::new(),
                decisions: Some(decisions),
            },
            &FactsConfig::default(),
        )
        .0;
        code_state = replay_fold(
            &session,
            code_state,
            &BTreeSet::new(),
            &FactsConfig::default(),
        )
        .facts;
        assert_eq!(
            active(&te_state),
            case.te_active_after_sessions[index],
            "{}: TE accepted filter, session {index}",
            case.source
        );
        assert_eq!(
            active(&code_state),
            case.code_only_active_after_sessions[index],
            "{}: documented code-only difference, session {index}",
            case.source
        );
    }
}

fn compare_loops(session: &Session, case: &Case) {
    let actual: Vec<_> = session
        .turns
        .iter()
        .map(|turn| {
            let mut guard = TurnGuard::new(EscalationMode::DelegatedNoAdapter);
            let mut output = LoopOracle {
                judge_calls: Vec::new(),
                backstop_calls: Vec::new(),
            };
            for (index, call) in turn.calls.iter().enumerate() {
                for action in guard.on_tool_done(call, &LoopConfig::default(), Some(8.0)) {
                    match action {
                        LoopAction::RequestJudge { .. } => output.judge_calls.push(index + 1),
                        LoopAction::Redirect { .. } => output.backstop_calls.push(index + 1),
                        other => panic!("unexpected action: {other:?}"),
                    }
                }
            }
            output
        })
        .collect();
    assert_eq!(actual, case.loops, "{}: all turns, envelope 8", case.source);
}

fn parity(host: &str, adapter: &dyn HostAdapter) {
    let path = root().join(format!("fixtures/golden/parity_{host}_oracle.json"));
    let oracle: Oracle = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    assert_eq!(oracle.te_commit, "d0675a3");
    assert_eq!(
        oracle.cases.len(),
        2,
        "recorded and positive synthetic logs required"
    );
    for case in oracle.cases {
        let session = adapter
            .read_session(&SessionSource {
                path: root().join(&case.source),
                cwd: "/synthetic/repo".into(),
                session_id: "parity".into(),
                repo_key: RepoKey("parity-repo".into()),
                agent_home: "/synthetic/agent".into(),
            })
            .unwrap();
        assert!(!session.partial, "{}: partial", case.source);
        assert_eq!(
            session
                .turns
                .iter()
                .map(|turn| turn.calls.len())
                .collect::<Vec<_>>(),
            case.calls_per_turn,
            "{}: native decoding",
            case.source
        );
        compare_facts(&session, &case);
        compare_loops(&session, &case);
    }
}

#[test]
fn parity_claude_fact_formation_and_loop_gate_match_te() {
    parity("claude", &ClaudeAdapter::new());
}
#[test]
fn parity_codex_fact_formation_and_loop_gate_match_te() {
    parity("codex", &CodexAdapter::new());
}
