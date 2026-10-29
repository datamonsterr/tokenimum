// Portions ported from microsoft/TokenEconomizer (tokeneconomizer/microskills/observe.py and lifecycle.py, commit d0675a3),
// Copyright (c) Microsoft Corporation, licensed under the Apache License, Version 2.0.
#![forbid(unsafe_code)]
#![allow(clippy::unwrap_used, clippy::expect_used)]
use std::collections::{BTreeMap, BTreeSet};
use tkm_core::{
    facts::{
        self,
        observe::{self, ErrorClass, Intent, Outcome},
        *,
    },
    model::{Call, EditKind, EditVia, FileEdit, Host, Session, ToolDef, Turn},
};
fn sh(cmd: &str, out: &str, code: i32) -> Call {
    Call {
        tool: "Bash".into(),
        args_digest: cmd.into(),
        output_digest: out.into(),
        exit_code: Some(code),
        ..Call::default()
    }
}
fn session(calls: Vec<Call>) -> Session {
    let mut s = Session::new(Host::Claude);
    s.session_id = "synthetic".into();
    s.turns = vec![Turn {
        calls,
        ..Turn::default()
    }];
    s
}
#[test]
fn rejects_whole_invalid_env_values_instead_of_learning_a_safe_prefix() {
    for value in [
        "conf.dev$(evil)",
        "conf.dev`evil`",
        "conf.dev'evil'",
        "conf.dev/../../secret",
    ] {
        let s = session(vec![
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
        ]);
        assert!(
            facts::extract::extract(&observe::observe(&s))
                .iter()
                .all(|c| c.family != Family::EnvValue),
            "{value}"
        );
    }
}
#[test]
fn quoted_env_values_remain_literal_and_validated() {
    for value in ["\"conf.dev\"", "'conf.dev'"] {
        let s = session(vec![
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
        ]);
        let cs = facts::extract::extract(&observe::observe(&s));
        assert!(
            cs.iter()
                .any(|c| c.family == Family::EnvValue && c.params.good.contains_key("conf.dev"))
        );
    }
}
#[test]
fn observation_error_precedence_table() {
    for (out, want) in [
        (
            "can't open file 'x': No such file or directory",
            ErrorClass::ScriptMissing,
        ),
        ("EACCES", ErrorClass::PermissionDenied),
        ("usage: test", ErrorClass::UsageError),
        ("ENOTFOUND", ErrorClass::NameResolution),
        ("ETIMEDOUT", ErrorClass::Unreachable),
        (
            "No module named pytest\ncommand not found",
            ErrorClass::ModuleMissing,
        ),
    ] {
        assert_eq!(observe::error_class(out), Some(want));
    }
}
#[test]
fn probe_kinds_and_hit_patterns_table() {
    for (cmd, out) in [
        ("git remote -v", "origin https://example.org (fetch)"),
        ("git branch -r", "  remotes/origin/main"),
        ("git tag --contains HEAD", "v1.2.3"),
        ("git fsck", "unreachable commit abcdef123"),
        ("git stash list", "stash@{0}: WIP"),
        ("git show-ref", "abcdef123 refs/heads/main"),
        ("git for-each-ref", "abcdef123 refs/heads/main"),
    ] {
        let d = observe::observe(&session(vec![sh(cmd, out, 0)]));
        assert_eq!(d.observations[0].outcome, Outcome::Hit, "{cmd}");
    }
}
#[test]
fn generic_test_summaries_and_all_edited_languages() {
    for (cmd, out) in [
        ("go test ./...", "ok  pkg 0.01s"),
        ("cargo test", "test result: ok. 2 passed"),
        ("rspec", "3 examples, 0 failures"),
        ("dotnet test", "Passed! - Passed: 3"),
        ("mvn test", "Tests run: 3"),
    ] {
        let d = observe::observe(&session(vec![sh(cmd, out, 0)]));
        assert_eq!(d.observations[0].intent, Intent::RunTests);
        assert!(d.observations[0].tests_ran);
    }
    let mut call = Call::default();
    for path in ["src/main.go", "web/app.ts", "pkg/m.py"] {
        call.edits.push(FileEdit {
            path: path.into(),
            kind: EditKind::Modify,
            via: EditVia::Tool,
        });
    }
    assert_eq!(
        observe::observe(&session(vec![call])).edited,
        vec!["app.ts", "m.py", "main.go"]
    );
}
#[test]
fn scope_is_inferred_from_host_tool_names() {
    let params = Params {
        bad_form: "functions.exec_command".into(),
        ..Default::default()
    };
    assert_eq!(
        facts::extract::scope_for(&params, Host::Codex, &["functions.exec_command".into()]),
        Scope::Host(Host::Codex)
    );
    assert_eq!(
        facts::extract::scope_for(&params, Host::Claude, &["Bash".into()]),
        Scope::Neutral
    );
    let mut s = session(vec![
        sh("pytest", "command not found", 127),
        sh("python -m pytest", "1 passed", 0),
    ]);
    s.tool_catalog = vec![ToolDef {
        name: "pytest".into(),
        ..ToolDef::default()
    }];
    assert_eq!(
        facts::extract::extract(&observe::observe(&s))[0].scope,
        Scope::Host(Host::Claude)
    );
}
#[test]
fn invalid_thresholds_use_code_only_and_invalid_config_abstains() {
    let id = "form_repair:run_tests:pytest";
    for thresholds in [(f64::NAN, 0.8), (0.9, 0.1)] {
        let mut state = FactsState::default();
        for i in 0..3 {
            let mut s = session(vec![
                sh("pytest", "command not found", 127),
                sh("python -m pytest", "1 passed", 0),
            ]);
            s.session_id = format!("s{i}");
            let e = SessionEvidence {
                session: s,
                injected_ids: BTreeSet::new(),
                decisions: Some(Decisions {
                    t_low: thresholds.0,
                    t_high: thresholds.1,
                    scores: BTreeMap::from([(
                        id.into(),
                        DecisionScores {
                            stable: Some(1.0),
                            safe: Some(1.0),
                            contradicts: None,
                        },
                    )]),
                }),
            };
            state = facts::update(state, &e, &FactsConfig::default()).0;
        }
        assert_eq!(state.facts[id].life.status, lifecycle::Status::Candidate);
    }
    let e = SessionEvidence {
        session: session(vec![
            sh("pytest", "command not found", 127),
            sh("python -m pytest", "1 passed", 0),
        ]),
        injected_ids: BTreeSet::new(),
        decisions: None,
    };
    for cfg in [
        FactsConfig {
            recency_decay: f64::NAN,
            ..Default::default()
        },
        FactsConfig {
            fire_gate: 2.0,
            ..Default::default()
        },
        FactsConfig {
            contradiction_weight: -1.0,
            ..Default::default()
        },
    ] {
        assert_eq!(
            facts::update(FactsState::default(), &e, &cfg),
            (FactsState::default(), Vec::new())
        );
    }
}
#[test]
fn boundary_success_with_failure_in_same_session_never_promotes() {
    let digest = observe::observe(&session(vec![
        sh("curl https://api.example.org", "Connection refused", 7),
        sh("curl https://api.example.org", "{}", 0),
    ]));
    let e = SessionEvidence {
        session: session(vec![
            sh("curl https://api.example.org", "Connection refused", 7),
            sh("curl https://api.example.org", "{}", 0),
        ]),
        injected_ids: BTreeSet::new(),
        decisions: None,
    };
    assert_eq!(facts::extract::extract(&digest).len(), 1);
    let (s, events) = facts::update(FactsState::default(), &e, &FactsConfig::default());
    assert_eq!(
        s.facts["boundary:fetch_remote"].life.status,
        lifecycle::Status::Retired
    );
    assert!(events.iter().any(|e| e.kind == FactEventKind::Contradicted));
    assert!(events.iter().any(|e| e.kind == FactEventKind::Retired));
}
