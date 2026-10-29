//! Outcome labels are observations, never inferred from the judged probability.
#![forbid(unsafe_code)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::pedantic)]
use std::collections::{BTreeMap, BTreeSet};
use tkm_core::{
    facts::*,
    model::{Call, Session, StopReason, Turn},
};
use tkm_decide::labels::{fact_outcome, question_labels, session_outcomes};

#[test]
fn fact_confirmation_requires_exercise_and_failure_overrides_confirmation() {
    assert_eq!(fact_outcome(false, true, false, false), None);
    assert_eq!(fact_outcome(true, true, false, false), Some(true));
    assert_eq!(fact_outcome(true, true, true, false), Some(false));
    assert_eq!(fact_outcome(true, true, false, true), Some(false));
    assert_eq!(fact_outcome(true, false, false, false), None);
}

fn example() -> (Session, FactsState) {
    let mut session = Session::new(tkm_core::model::Host::Claude);
    session.turns.push(Turn {
        stopped_by: StopReason::OwnStop,
        calls: vec![Call {
            tool: "Bash".into(),
            args_digest: "python -m pytest".into(),
            output_digest: "10 passed".into(),
            exit_code: Some(0),
            ..Call::default()
        }],
        ..Turn::default()
    });
    let fact = Fact {
        candidate: Candidate {
            id: "repair".into(),
            family: Family::FormRepair,
            params: Params {
                intent: Some(observe::Intent::RunTests),
                bad_form: "pytest".into(),
                forms: BTreeMap::from([("python -m pytest".into(), 3)]),
                ..Params::default()
            },
            scope: Scope::Neutral,
            evidence: vec![],
        },
        life: lifecycle::Lifecycle::default(),
    };
    (
        session,
        FactsState {
            facts: BTreeMap::from([("repair".into(), fact)]),
            ..FactsState::default()
        },
    )
}

#[test]
fn normalized_sessions_label_held_failed_and_unexercised_facts() {
    let (mut session, state) = example();
    let injected = BTreeSet::from(["repair".into()]);
    assert_eq!(
        session_outcomes(&session, &state, &injected).get("repair"),
        Some(&true)
    );
    session.turns[0].calls[0].exit_code = Some(1);
    session.turns[0].calls[0].output_digest = "ImportError: missing dependency".into();
    session.turns[0].calls[0].is_error = true;
    assert_eq!(
        session_outcomes(&session, &state, &injected).get("repair"),
        Some(&false)
    );
    session.turns[0].calls.clear();
    assert!(session_outcomes(&session, &state, &injected).is_empty());
    session.partial = true;
    assert!(session_outcomes(&session, &state, &injected).is_empty());
}

#[test]
fn batch_labels_preserve_item_identity_and_contradiction_polarity() {
    let observations = BTreeMap::from([("a".into(), true), ("b".into(), false)]);
    let questions = [
        "stable_a",
        "safe_b",
        "contradicts_a",
        "contradicts_b",
        "rel_a",
        "rel_c",
    ];
    let labels = question_labels(questions.into_iter(), &observations);
    assert_eq!(
        labels,
        BTreeMap::from([
            ("stable_a".into(), true),
            ("safe_b".into(), false),
            ("contradicts_a".into(), false),
            ("contradicts_b".into(), true),
            ("rel_a".into(), true),
        ])
    );
}

#[test]
fn relevance_labels_require_an_exercised_skill_and_observed_outcome() {
    let (mut session, _) = example();
    session.turns[0].calls = vec![Call {
        tool: "Skill".into(),
        args_digest: r#"{"skill":"review"}"#.into(),
        exit_code: Some(0),
        ..Call::default()
    }];
    let labels = session_outcomes(&session, &FactsState::default(), &BTreeSet::new());
    assert_eq!(labels.get("skill:review"), Some(&true));
    session.turns[0].calls[0].is_error = true;
    let labels = session_outcomes(&session, &FactsState::default(), &BTreeSet::new());
    assert_eq!(labels.get("skill:review"), Some(&false));
}
