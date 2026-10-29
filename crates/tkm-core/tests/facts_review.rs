// Portions ported from microsoft/TokenEconomizer (tokeneconomizer/microskills/observe.py and envfacts.py, commit d0675a3),
// Copyright (c) Microsoft Corporation, licensed under the Apache License, Version 2.0.
#![forbid(unsafe_code)]
#![allow(clippy::unwrap_used, clippy::expect_used)]
use std::collections::{BTreeMap, BTreeSet};
use tkm_core::{
    facts::{
        self,
        observe::{self, ErrorClass, Intent},
        *,
    },
    model::{Call, Host, Session, ToolDef, Turn},
};
const ID: &str = "form_repair:run_tests:pytest";
fn sh(command: &str, output: &str, code: i32) -> Call {
    Call {
        tool: "Bash".into(),
        args_digest: command.into(),
        output_digest: output.into(),
        exit_code: Some(code),
        ..Call::default()
    }
}
fn repaired() -> Vec<Call> {
    vec![
        sh("pytest -q", "command not found", 127),
        sh("python -m pytest -q", "4 passed", 0),
    ]
}
fn input(id: &str, calls: Vec<Call>, stable: f64, safe: f64) -> SessionEvidence {
    let mut session = Session::new(Host::Claude);
    session.session_id = id.into();
    session.agent_home = "/agents/claude".into();
    session.turns = vec![Turn {
        calls,
        ..Turn::default()
    }];
    SessionEvidence {
        session,
        injected_ids: BTreeSet::new(),
        decisions: Some(Decisions {
            t_low: 0.3,
            t_high: 0.8,
            scores: BTreeMap::from([(
                ID.into(),
                DecisionScores {
                    stable: Some(stable),
                    safe: Some(safe),
                    contradicts: None,
                },
            )]),
        }),
    }
}
fn formed() -> FactsState {
    (0..4).fold(FactsState::default(), |state, i| {
        facts::update(
            state,
            &input(&format!("seed-{i}"), repaired(), 1.0, 1.0),
            &FactsConfig::default(),
        )
        .0
    })
}
fn candidate() -> Candidate {
    Candidate {
        id: ID.into(),
        family: Family::FormRepair,
        params: Params {
            intent: Some(Intent::RunTests),
            bad_form: "pytest".into(),
            forms: BTreeMap::from([("python -m pytest".into(), 1)]),
            errors: BTreeMap::from([(ErrorClass::CommandMissing, 1)]),
            ..Params::default()
        },
        scope: Scope::Neutral,
        evidence: Vec::new(),
    }
}
#[test]
fn review_rejected_scores_never_confirm_an_existing_candidate() {
    for (stable, safe) in [(0.1, 1.0), (1.0, 0.1), (0.1, 0.1)] {
        let mut state = facts::update(
            FactsState::default(),
            &input("accepted", repaired(), 1.0, 1.0),
            &FactsConfig::default(),
        )
        .0;
        for i in 0..3 {
            let (next, events) = facts::update(
                state,
                &input(&format!("rejected-{i}"), repaired(), stable, safe),
                &FactsConfig::default(),
            );
            state = next;
            assert!(
                !events
                    .iter()
                    .any(|event| event.kind == FactEventKind::Confirmed)
            );
        }
        assert_eq!(state.facts[ID].life.confirmations, 1);
        assert_eq!(state.facts[ID].life.status, lifecycle::Status::Candidate);
        assert_eq!(
            state.facts[ID].candidate.params.forms["python -m pytest"],
            1
        );
    }
}
#[test]
fn review_rejected_filter_scores_still_allow_code_contradictions() {
    let (state, events) = facts::update(
        formed(),
        &input(
            "rejected-contradiction",
            vec![sh("python -m pytest -q", "No module named pytest", 1)],
            0.1,
            0.1,
        ),
        &FactsConfig::default(),
    );
    assert_eq!(state.facts[ID].life.contradictions, 1);
    assert!(
        events
            .iter()
            .any(|event| event.kind == FactEventKind::Contradicted)
    );
}
#[test]
fn review_segments_respect_unquoted_escapes_and_shell_quote_rules() {
    for command in [
        r"echo 4 passed\;python -m pytest",
        r"echo 4 passed\|python -m pytest",
        r"echo 4 passed\&\&python -m pytest",
        r#"echo '4 passed;python -m pytest'"#,
        r#"echo "4 passed\";python -m pytest""#,
    ] {
        assert_eq!(observe::segments(command), vec![command]);
        assert_eq!(
            observe::classify_command(command),
            Vec::<observe::Observation>::new()
        );
    }
    for (command, first) in [
        (r#"echo escaped\"; python -m pytest"#, r#"echo escaped\""#),
        (r"echo 'literal\'; python -m pytest", r"echo 'literal\'"),
    ] {
        assert_eq!(observe::segments(command), vec![first, "python -m pytest"]);
        let observed = observe::classify_command(command);
        assert_eq!(observed.len(), 1);
        assert_eq!(observed[0].intent, Intent::RunTests);
    }
}
#[test]
fn review_assignment_parser_preserves_complete_concatenated_words() {
    for (value, expected) in [
        (r#""conf.dev".prod"#, "conf.dev.prod"),
        (r#"conf."dev""#, "conf.dev"),
        ("'conf'.dev", "conf.dev"),
    ] {
        let event = input(
            "assignment",
            vec![
                sh(
                    "python -c 'import app'",
                    "environment variable APP_SETTINGS",
                    1,
                ),
                sh(
                    &format!("APP_SETTINGS={value} python -c 'import app'"),
                    "ran",
                    0,
                ),
            ],
            1.0,
            1.0,
        );
        let observed = observe::observe(&event.session);
        assert_eq!(
            observed.env[1].sets.get("APP_SETTINGS").map(String::as_str),
            Some(expected)
        );
        let candidates = facts::extract::extract(&observed);
        let found = candidates
            .iter()
            .find(|c| c.family == Family::EnvValue)
            .unwrap();
        assert_eq!(found.params.good, BTreeMap::from([(expected.into(), 1)]));
    }
}
#[test]
fn review_assignment_substitutions_and_quoted_prose_cannot_teach_values() {
    for command in [
        r#"APP_SETTINGS="conf.dev"$(printf .prod) python -c 'import app'"#,
        r#"APP_SETTINGS="conf.dev"`printf .prod` python -c 'import app'"#,
        r#"APP_SETTINGS="conf.dev"${SUFFIX} python -c 'import app'"#,
        r#"APP_SETTINGS=$(printf 'APP_SETTINGS=conf.dev') python -c 'import app'"#,
        "echo 'APP_SETTINGS=conf.dev'",
        "python -c 'print(\"APP_SETTINGS=conf.dev\")'",
    ] {
        let event = input(
            "substitution",
            vec![
                sh(
                    "python -c 'import app'",
                    "environment variable APP_SETTINGS",
                    1,
                ),
                sh(command, "ran", 0),
            ],
            1.0,
            1.0,
        );
        let observed = observe::observe(&event.session);
        assert!(
            facts::extract::extract(&observed)
                .iter()
                .all(|candidate| candidate.family != Family::EnvValue),
            "{command}"
        );
    }
}
#[test]
fn review_working_forms_are_validated_and_scoped_before_candidate_filtering() {
    let mut event = input("scope", repaired(), 1.0, 1.0);
    event.session.tool_catalog = vec![ToolDef {
        name: "python".into(),
        ..ToolDef::default()
    }];
    let observed = observe::observe(&event.session);
    let candidates = facts::extract::extract(&observed);
    let found = &candidates[0];
    assert_eq!(
        found.params.forms,
        BTreeMap::from([("python -m pytest".into(), 1)])
    );
    assert_eq!(found.scope, Scope::Host(Host::Claude));
    assert!(lint::validate(found));
    for repair in [
        "/outside/bin/python -m pytest",
        "C:../outside/python -m pytest",
        "python -m pytest",
    ] {
        let event = input(
            "validation",
            vec![
                sh("pytest", "command not found", 127),
                sh(repair, "1 passed", 0),
            ],
            1.0,
            1.0,
        );
        let candidates = facts::extract::extract(&observe::observe(&event.session));
        if repair == "python -m pytest" {
            assert_eq!(candidates.len(), 1);
        } else {
            assert_eq!(candidates, Vec::<Candidate>::new());
        }
    }
}
#[test]
fn review_lint_and_command_validation_reject_file_uris_and_windows_paths() {
    for path in [
        "file:///etc/python",
        "FILE:///etc/python",
        "C:../outside/python",
        r"X:\outside\python",
        r"X:\",
        r"\\server\share\python",
        "C:outside/python",
    ] {
        assert!(!lint::lint_ok(path), "{path}");
        let mut fact = candidate();
        fact.params.forms = BTreeMap::from([(format!("{path} -m pytest"), 1)]);
        assert!(!lint::validate(&fact), "{path}");
        assert_eq!(render::claim(&fact), "");
    }
}
#[test]
fn review_django_manage_test_forms_are_canonical_and_renderable() {
    for command in [
        "python manage.py test app.tests -q",
        "./manage.py test app.tests",
        "python project/manage.py test app.tests",
    ] {
        let observed = observe::classify_command(command);
        assert_eq!(observed.len(), 1);
        let o = &observed[0];
        assert_eq!(o.intent, Intent::RunTests);
        assert!(o.form.ends_with("manage.py test"));
        assert_eq!(o.labels, vec!["app.tests"]);
        let mut fact = candidate();
        fact.params.forms = BTreeMap::from([(o.form.clone(), 1)]);
        assert!(lint::validate(&fact));
        assert!(render::claim(&fact).contains(&o.form));
    }
}
#[test]
fn review_replay_identity_includes_host_and_exact_agent_home() {
    let cfg = FactsConfig::default();
    let first = input("same-native-id", repaired(), 1.0, 1.0);
    let mut state = facts::update(FactsState::default(), &first, &cfg).0;
    let mut second = first.clone();
    second.session.host = Host::Codex;
    state = facts::update(state, &second, &cfg).0;
    assert_eq!(state.facts[ID].life.confirmations, 2);
    second.session.agent_home = "/agents/codex-other".into();
    state = facts::update(state, &second, &cfg).0;
    assert_eq!(state.facts[ID].life.confirmations, 3);
    let json = serde_json::to_string(&state).unwrap();
    let reloaded: FactsState = serde_json::from_str(&json).unwrap();
    assert_eq!(facts::update(reloaded.clone(), &second, &cfg).0, reloaded);
}
#[test]
fn review_malformed_contradiction_scores_are_ignored() {
    for score in [-0.1, 1.01, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        let mut event = input(
            "malformed-contradiction",
            vec![sh("python -m pytest -q", "1 passed", 0)],
            1.0,
            1.0,
        );
        event
            .decisions
            .as_mut()
            .unwrap()
            .scores
            .get_mut(ID)
            .unwrap()
            .contradicts = Some(score);
        let (state, events) = facts::update(formed(), &event, &FactsConfig::default());
        assert_eq!(state.facts[ID].life.contradictions, 0, "{score}");
        assert!(
            !events
                .iter()
                .any(|event| event.kind == FactEventKind::Contradicted)
        );
    }
}
