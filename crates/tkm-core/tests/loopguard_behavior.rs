// Portions ported from microsoft/TokenEconomizer (tests/test_live_core.py, experiments/tests/test_spiral.py, commit d0675a3),
// Copyright (c) Microsoft Corporation, licensed under the Apache License, Version 2.0.
#![forbid(unsafe_code)]
#![allow(clippy::unwrap_used, clippy::expect_used)]
use tkm_core::loopguard::shell_edits::shell_edit_targets;
use tkm_core::loopguard::*;
use tkm_core::model::{Call, EditKind, EditVia, FileEdit, Host, StopReason, Turn};

fn shell(command: &str, output: &str, exit_code: Option<i32>) -> Call {
    Call {
        tool: "Bash".into(),
        args_digest: command.into(),
        output_digest: output.into(),
        exit_code,
        ..Call::default()
    }
}
fn edit() -> Call {
    Call {
        tool: "Edit".into(),
        edits: vec![FileEdit {
            path: "src/a.py".into(),
            kind: EditKind::Modify,
            via: EditVia::Tool,
        }],
        ..Call::default()
    }
}
fn fail() -> Call {
    shell("pytest -q", "ImportError: missing module", Some(1))
}
fn thresholds() -> JudgeThresholds {
    JudgeThresholds {
        t_stuck: 0.8,
        t_progress: 0.2,
        t_low: 0.3,
    }
}
fn request(actions: &[LoopAction]) -> Option<JudgeRequest> {
    actions.iter().find_map(|a| {
        if let LoopAction::RequestJudge { request } = a {
            Some(*request)
        } else {
            None
        }
    })
}
fn warmed_guard() -> (TurnGuard, JudgeRequest) {
    let mut g = TurnGuard::new(EscalationMode::DelegatedNoAdapter);
    let cfg = LoopConfig::default();
    let mut found = None;
    for _ in 0..18 {
        found = request(&g.on_tool_done(&fail(), &cfg, Some(100.0))).or(found);
    }
    (g, found.unwrap())
}

#[test]
fn golden_loop_fixture_wakes_gate_and_backstop() {
    let calls: Vec<Call> =
        serde_json::from_str(include_str!("../../../fixtures/loopguard/import_loop.json")).unwrap();
    let mut g = TurnGuard::new(EscalationMode::DelegatedNoAdapter);
    let mut judges = Vec::new();
    let mut redirects = Vec::new();
    for (index, call) in calls.iter().enumerate() {
        for action in g.on_tool_done(call, &LoopConfig::default(), Some(8.0)) {
            match action {
                LoopAction::RequestJudge { .. } => judges.push(index + 1),
                LoopAction::Redirect {
                    redirect_kind,
                    text,
                } => {
                    assert_eq!(redirect_kind, RedirectKind::rule_default());
                    assert!(text.starts_with("tokenimum:"));
                    redirects.push(index + 1);
                }
                LoopAction::Escalate { .. } => panic!("first stuck segment must redirect"),
            }
        }
    }
    assert_eq!(judges, vec![18]);
    assert_eq!(redirects, vec![21]); // strict > 2.5 * 8; call 20 is protected
    assert_eq!(g.whole.turns, 21);
    assert_eq!(g.whole.edit_churn(), 4);
    assert_eq!(g.whole.error_outputs, 6);
    assert_eq!(g.whole.test_runs, 6);
    assert_eq!(g.segment.turns, 0);
}

#[test]
fn shell_edit_table_matches_te_and_rejects_reads_and_output_captures() {
    let cases: &[(&str, &[&str])] = &[
        ("sed -i 's/a/b/' pkg/mod.py", &["mod.py"]),
        (
            "cd /t && sed -i.bak -e 's/a/b/' -e 's/c/d/' x.py y.py && pytest -q",
            &["x.py", "y.py"],
        ),
        ("sed -i '' 's/a/b/' x.py", &["x.py"]),
        ("perl -pi -e 's/foo/bar/' src/a.rs", &["a.rs"]),
        (
            "cat > repro.py <<'EOF'\nprint(1 > 0)\nEOF\npython repro.py",
            &["repro.py"],
        ),
        ("echo x >> src/a.rs", &["a.rs"]),
        ("echo x | tee -a setup.cfg", &["setup.cfg"]),
        (
            "python - <<'E'\np='sympy/printing/str.py'; s=open(p).read()\nopen(p,'w').write(s)\nE",
            &["str.py"],
        ),
        (
            "python - <<'E'\nfor p in ['x/a.py', 'x/b.py']:\n    open(p, 'w').write('')\nE",
            &["a.py", "b.py"],
        ),
        (
            "python - <<'E'\nfrom pathlib import Path\nPath('m/n.py').write_text('x')\nE",
            &["n.py"],
        ),
        ("python -c \"open('src/a.rs', 'a').write('x')\"", &["a.rs"]),
        ("python -c \"open(target, 'w').write('x')\"", &["(script)"]),
        (
            "python -c \"p=Path('a.py'); p.write_bytes(b'x')\"",
            &["a.py"],
        ),
        ("git apply /tmp/fix.diff", &["(patch)"]),
        ("patch -p1 < /tmp/fix.diff", &["(patch)"]),
        (
            "apply_patch <<'P'\n*** Begin Patch\n*** Add File: a/new.py\n+x\n*** Update File: b/old.py\n*** End Patch\nP",
            &["new.py", "old.py"],
        ),
        (
            "/bin/bash -lc \"apply_patch <<'P'\n*** Begin Patch\n*** Update File: pkg/a.py\n@@\n-x\n+y\n*** End Patch\nP\n git diff --check\"",
            &["a.py"],
        ),
        ("git checkout -- a.py b/c.py", &["a.py", "c.py"]),
        ("git restore a.py", &["a.py"]),
        (
            "python -m pytest -q > /tmp/out.txt 2>&1; tail -5 /tmp/out.txt",
            &[],
        ),
        ("grep -n x a.py > hits.txt", &[]),
        ("grep -n '>' x.py", &[]),
        ("grep -nE 'a|b' x.py | head", &[]),
        ("python -c \"print(1>2)\"", &[]),
        ("python -c \"open('a.py', 'r').read()\"", &[]),
        ("git stash -q; python bin/test x; git stash pop -q", &[]),
        ("sed -n '1,20p' pkg/mod.py", &[]),
        ("echo x > /dev/null 2>&1", &[]),
        (
            "sed -i -E 's/x (<|<=|>|>=) y/z/' numbers.py expr.py",
            &["numbers.py", "expr.py"],
        ),
        ("sed -i 's/a/b/' x.py 2>&1 | tail -2", &["x.py"]),
        ("env FOO=1 command sed -i 's/a/b/' a.py a.py", &["a.py"]),
        ("echo x > /tmp/a.py; echo x | tee /proc/a.py", &[]),
        ("echo 'unterminated\nsed -i 's/a/b/' a.py", &["a.py"]),
        ("", &[]),
    ];
    for (command, want) in cases {
        assert_eq!(shell_edit_targets(command), *want, "{command}");
    }
}

#[test]
fn state_folds_edits_once_and_normalizes_repeat_failures() {
    let mut state = SpiralState::default();
    let mut call = shell("sed -i 's/a/b/' src/a.py", "", Some(0));
    call.edits = edit().edits;
    state.update(&call);
    state.update(&shell(
        "pytest",
        "ImportError: line 42 at /tmp/one.py 0xaffe",
        Some(1),
    ));
    state.update(&shell(
        "pytest",
        "ImportError: line 99 at /other/two.py 0xbeef",
        None,
    ));
    assert_eq!(state.turns, 3);
    assert_eq!(state.edit_churn(), 1);
    assert_eq!(state.distinct_edit_files(), 1);
    assert_eq!(state.error_outputs, 2);
    assert_eq!(state.repeated_fail_sigs, 1);
    assert!((state.stagnation() - 0.5).abs() < f64::EPSILON);
    assert!(
        !serde_json::to_string(&state)
            .unwrap()
            .contains("ImportError")
    );
}

#[test]
fn content_reads_and_successful_commands_are_not_failures() {
    let mut state = SpiralState::default();
    for tool in ["Read", "view", "Grep", "Glob"] {
        state.update(&Call {
            tool: tool.into(),
            output_digest: "assert x; ValueError".into(),
            ..Call::default()
        });
    }
    state.update(&shell(
        "cat tests/test_x.py",
        "assert x; ValueError",
        Some(0),
    ));
    assert_eq!(state.error_outputs, 0);
    state.update(&Call {
        tool: "Read".into(),
        output_digest: "Error: missing file".into(),
        is_error: true,
        ..Call::default()
    });
    assert_eq!(state.error_outputs, 1);
    assert_eq!(state.turns, 6);
}

#[test]
fn rule_conjunction_and_gate_boundaries_are_table_driven() {
    let cfg = LoopConfig::default();
    let base = SpiralState {
        turns: 20,
        edit_counts: std::collections::BTreeMap::from([("a.py".into(), 4)]),
        error_outputs: 6,
        fail_sigs: std::collections::BTreeMap::from([("x".into(), 5)]),
        repeated_fail_sigs: 1,
        ..SpiralState::default()
    };
    for (turns, churn, errors, repeats, want) in [
        (20, 4, 6, 1, true),
        (19, 4, 6, 1, false),
        (20, 3, 6, 1, false),
        (20, 4, 5, 1, false),
        (20, 4, 6, 0, false),
    ] {
        let mut s = base.clone();
        s.turns = turns;
        s.edit_counts.insert("a.py".into(), churn);
        s.error_outputs = errors;
        s.repeated_fail_sigs = repeats;
        assert_eq!(is_stuck(&s, &cfg), want);
    }
    let mut scatter = base.clone();
    scatter.edit_counts =
        std::collections::BTreeMap::from([("a".into(), 1), ("b".into(), 1), ("c".into(), 1)]);
    assert!(is_stuck(&scatter, &cfg));
    for (turns, repeats, errors, want) in [
        (17, 1, 0, false),
        (18, 1, 0, true),
        (18, 0, 7, false),
        (18, 0, 8, true),
        (50, 0, 0, false),
    ] {
        let s = SpiralState {
            turns,
            repeated_fail_sigs: repeats,
            error_outputs: errors,
            ..SpiralState::default()
        };
        assert_eq!(gate_for_judge(&s, &cfg), want);
    }
}

#[test]
fn presets_match_every_te_override_and_unknown_falls_back() {
    let d = LoopConfig::default();
    assert_eq!(LoopConfig::preset("default"), d);
    assert_eq!(LoopConfig::preset("unknown"), d);
    let c = LoopConfig {
        judge_confidence: 0.85,
        envelope_margin: 1.5,
        backstop_mult: 3.5,
        stagnation: 0.30,
        gate_min_turns: 24,
        min_turns: 28,
        edit_churn: 5,
        error_outputs: 8,
        envelope_min_samples: 7,
        ..d.clone()
    };
    let a = LoopConfig {
        judge_confidence: 0.55,
        envelope_margin: 1.0,
        backstop_mult: 1.8,
        stagnation: 0.12,
        gate_min_turns: 12,
        min_turns: 14,
        edit_churn: 3,
        error_outputs: 4,
        envelope_min_samples: 3,
        nudge_on_ambiguous: true,
        ..d
    };
    assert_eq!(LoopConfig::preset("conservative"), c);
    assert_eq!(LoopConfig::preset("aggressive"), a);
}

#[test]
fn score_combiner_boundaries_and_invalid_inputs_fail_open() {
    for (stuck, progress, want) in [
        (0.8, 0.2, JudgeVerdict::Stuck),
        (0.9, 0.21, JudgeVerdict::Wait),
        (0.3, 0.9, JudgeVerdict::NotStuck),
        (0.5, 0.0, JudgeVerdict::Wait),
        (f64::NAN, 0.0, JudgeVerdict::Wait),
        (1.1, 0.0, JudgeVerdict::Wait),
        (0.9, -0.1, JudgeVerdict::Wait),
    ] {
        assert_eq!(
            combine_verdict(JudgeScores { stuck, progress }, thresholds()),
            want
        );
    }
    assert_eq!(
        combine_verdict(
            JudgeScores {
                stuck: 0.9,
                progress: 0.1
            },
            JudgeThresholds {
                t_low: 0.9,
                ..thresholds()
            }
        ),
        JudgeVerdict::Wait
    );
}

#[test]
fn no_envelope_or_invalid_envelope_never_produces_actions() {
    for envelope in [
        None,
        Some(f64::NAN),
        Some(f64::INFINITY),
        Some(0.0),
        Some(-1.0),
    ] {
        let mut g = TurnGuard::new(EscalationMode::DelegatedNoAdapter);
        for _ in 0..80 {
            assert!(
                g.on_tool_done(&fail(), &LoopConfig::default(), envelope)
                    .is_empty()
            );
        }
        assert_eq!(g.whole.turns, 80);
    }
}

#[test]
fn judge_single_flight_rate_limit_and_failure_release() {
    let (mut g, first) = warmed_guard();
    for _ in 0..4 {
        assert!(
            g.on_tool_done(&fail(), &LoopConfig::default(), Some(100.0))
                .is_empty()
        );
    }
    g.on_judge_unavailable(first);
    let second = request(&g.on_tool_done(&fail(), &LoopConfig::default(), Some(100.0))).unwrap();
    assert_eq!(second.call_index, 23);
    assert_eq!(
        g.on_judge_verdict(
            first,
            JudgeScores {
                stuck: 1.0,
                progress: 0.0
            },
            thresholds(),
            RedirectKind::AskUser
        ),
        JudgeVerdict::Wait
    );
    assert_eq!(
        g.on_judge_verdict(
            second,
            JudgeScores {
                stuck: 0.1,
                progress: 0.9
            },
            thresholds(),
            RedirectKind::CheckEnv
        ),
        JudgeVerdict::NotStuck
    );
    for _ in 0..4 {
        assert!(
            g.on_tool_done(&fail(), &LoopConfig::default(), Some(100.0))
                .is_empty()
        );
    }
    assert_eq!(
        request(&g.on_tool_done(&fail(), &LoopConfig::default(), Some(100.0)))
            .unwrap()
            .call_index,
        28
    );
}

#[test]
fn verdict_redirect_is_delivered_on_next_call_and_starts_fresh_segment() {
    let (mut g, r) = warmed_guard();
    assert_eq!(
        g.on_judge_verdict(
            r,
            JudgeScores {
                stuck: 0.9,
                progress: 0.1
            },
            thresholds(),
            RedirectKind::CheckEnv
        ),
        JudgeVerdict::Stuck
    );
    assert_eq!(g.redirects_done, 0);
    let actions = g.on_tool_done(
        &shell("pwd", "/repo", Some(0)),
        &LoopConfig::default(),
        Some(100.0),
    );
    assert!(
        matches!(&actions[..],[LoopAction::Redirect { redirect_kind:RedirectKind::CheckEnv,text }] if text.contains("ImportError: missing module") && text.contains("pytest -q"))
    );
    assert_eq!(
        (
            g.segment_id,
            g.segment.turns,
            g.whole.turns,
            g.redirects_done
        ),
        (1, 0, 19, 1)
    );
    assert_eq!(
        g.on_judge_verdict(
            r,
            JudgeScores {
                stuck: 1.0,
                progress: 0.0
            },
            thresholds(),
            RedirectKind::AskUser
        ),
        JudgeVerdict::Wait
    );
    for _ in 0..17 {
        assert!(
            g.on_tool_done(&fail(), &LoopConfig::default(), Some(100.0))
                .is_empty()
        );
    }
    assert!(request(&g.on_tool_done(&fail(), &LoopConfig::default(), Some(100.0))).is_some());
}

#[test]
fn small_envelope_redirects_then_escalates_once_for_each_delivery_mode() {
    for mode in [
        EscalationMode::Interactive {
            stop_key: "Esc".into(),
        },
        EscalationMode::DelegatedWithAdapter,
        EscalationMode::DelegatedNoAdapter,
    ] {
        let mut g = TurnGuard::new(mode.clone());
        let mut actions = Vec::new();
        for i in 1..=70 {
            let call = if i % 2 == 0 { fail() } else { edit() };
            for a in g.on_tool_done(&call, &LoopConfig::preset("aggressive"), Some(4.0)) {
                if !matches!(a, LoopAction::RequestJudge { .. }) {
                    actions.push((i, a));
                }
            }
        }
        assert!(
            matches!(&actions[..],[(14,LoopAction::Redirect{..}),(28,LoopAction::Escalate{mode:m})] if m==&mode)
        );
        assert_eq!(g.whole.turns, 70);
    }
}

#[test]
fn every_action_is_advisory_and_state_round_trips_mid_judge() {
    let (g, r) = warmed_guard();
    let mut restored: TurnGuard =
        serde_json::from_str(&serde_json::to_string(&g).unwrap()).unwrap();
    assert_eq!(g, restored);
    restored.on_judge_verdict(
        r,
        JudgeScores {
            stuck: 1.0,
            progress: 0.0,
        },
        thresholds(),
        RedirectKind::RereadError,
    );
    for action in restored.on_tool_done(&fail(), &LoopConfig::default(), Some(100.0)) {
        match action {
            LoopAction::RequestJudge { .. }
            | LoopAction::Redirect { .. }
            | LoopAction::Escalate { .. } => {}
        }
    }
    for host in [
        Host::Claude,
        Host::Codex,
        Host::Agy,
        Host::Cursor,
        Host::Kimi,
        Host::Deepseek,
        Host::Opencode,
        Host::Pi,
    ] {
        assert_eq!(stop_key(host), "Esc");
    }
}

#[test]
fn redirect_library_has_exact_wire_names_and_safe_template_renderings() {
    let cases = [
        (RedirectKind::RereadError, "reread_error", "Re-read"),
        (RedirectKind::CheckEnv, "check_env", "Check the environment"),
        (RedirectKind::SmallerStep, "smaller_step", "smaller"),
        (RedirectKind::RevertLastEdit, "revert_last_edit", "Revert"),
        (
            RedirectKind::DifferentTest,
            "different_test",
            "different test",
        ),
        (RedirectKind::ReadMoreCode, "read_more_code", "Read"),
        (RedirectKind::AskUser, "ask_user", "Ask"),
    ];
    for (kind, name, phrase) in cases {
        assert_eq!(kind.as_str(), name);
        assert_eq!(name.parse::<RedirectKind>().unwrap(), kind);
        assert_eq!(serde_json::to_string(&kind).unwrap(), format!("\"{name}\""));
        let text = kind.render(&RedirectParams {
            calls: 18,
            failing_command: "pytest -q",
            error_line: "ImportError: missing",
        });
        assert!(text.starts_with("tokenimum:"));
        assert!(text.contains(phrase), "{text}");
        assert!(text.contains("pytest -q"));
        assert!(text.contains("ImportError: missing"));
        assert!(text.contains("18"));
        let hostile = format!("`bad`\n\x1b[31m{}", "é".repeat(300));
        let text = kind.render(&RedirectParams {
            calls: 18,
            failing_command: &hostile,
            error_line: &hostile,
        });
        assert!(!text.contains('`'));
        assert!(!text.chars().any(char::is_control));
        assert!(!text.contains(&"é".repeat(161)));
    }
    for bad in ["RereadError", "reread-error", "unknown", ""] {
        assert!(bad.parse::<RedirectKind>().is_err());
    }
}

#[test]
fn good_turn_classifier_and_envelope_use_ended_green_only() {
    let own = |calls| Turn {
        calls,
        stopped_by: StopReason::OwnStop,
        ..Turn::default()
    };
    let cases = [
        (own(vec![edit()]), false, true),
        (own(vec![edit(), fail()]), false, false),
        (own(vec![fail(), edit()]), false, true),
        (
            own(vec![
                edit(),
                fail(),
                shell("pytest", "1 passed in 0.3s", Some(0)),
            ]),
            false,
            true,
        ),
        (own(vec![edit()]), true, false),
        (
            Turn {
                stopped_by: StopReason::Interrupted,
                ..own(vec![edit()])
            },
            false,
            false,
        ),
        (own(vec![]), false, false),
        (
            own(vec![shell(
                "sed -i 's/a/b/' a.py && pytest -q",
                "ImportError",
                Some(1),
            )]),
            false,
            false,
        ),
    ];
    let mut tracker = EnvelopeTracker::default();
    for (turn, escalated, want) in cases {
        assert_eq!(is_good_turn(&turn, escalated), want);
        assert_eq!(tracker.record(&turn, escalated), want);
    }
    assert_eq!(tracker.lengths(), &[1, 2, 3]);
}

#[test]
fn envelope_interpolates_p90_and_retains_last_500_good_turns() {
    let cfg = LoopConfig::default();
    let mut tracker = EnvelopeTracker::default();
    for n in [10, 20, 30, 40] {
        tracker.record(
            &Turn {
                calls: vec![Call::default(); n],
                stopped_by: StopReason::OwnStop,
                ..Turn::default()
            },
            false,
        );
        assert_eq!(tracker.value(&cfg), None);
    }
    tracker.record(
        &Turn {
            calls: vec![Call::default(); 100],
            stopped_by: StopReason::OwnStop,
            ..Turn::default()
        },
        false,
    );
    assert!((tracker.value(&cfg).unwrap() - 95.0).abs() < 1e-10); // (40 + .6*60) * 1.25
    for _ in 0..500 {
        tracker.record(
            &Turn {
                calls: vec![Call::default(); 2],
                stopped_by: StopReason::OwnStop,
                ..Turn::default()
            },
            false,
        );
    }
    assert_eq!(tracker.lengths().len(), 500);
    assert!(tracker.lengths().iter().all(|&n| n == 2));
    assert!((tracker.value(&cfg).unwrap() - 2.5).abs() < f64::EPSILON);
    let restored: EnvelopeTracker =
        serde_json::from_str(&serde_json::to_string(&tracker).unwrap()).unwrap();
    assert_eq!(restored, tracker);
}

#[test]
fn verification_paths_hidden_status_and_runner_summaries_match_te() {
    for command in [
        "python tests/runtests.py model_inheritance -v 2",
        "./runtests.py basic",
        ".venv/bin/pytest -q tests/test_a.py",
        "cd web && ./node_modules/.bin/jest src",
        "/usr/bin/python3 -m pytest",
        "/w/venv/bin/pytest -q",
        "~/.local/bin/pytest -q",
        "python3 /w/repo/tests/runtests.py basic",
        "node --test test/",
        "cargo test",
        "rtk cargo test",
    ] {
        let turn = Turn {
            calls: vec![edit(), shell(command, "FAILED (failures=1)", Some(1))],
            stopped_by: StopReason::OwnStop,
            ..Turn::default()
        };
        assert!(!is_good_turn(&turn, false), "{command}");
    }
    for command in [
        "cat /w/pytest.ini",
        "cat pytest.ini",
        "ls /w/tests",
        "vim tests/runtests.py.bak",
        "pytest --collect-only",
        "pytest --help",
    ] {
        let turn = Turn {
            calls: vec![edit(), shell(command, "FAILED (failures=1)", Some(1))],
            stopped_by: StopReason::OwnStop,
            ..Turn::default()
        };
        assert!(is_good_turn(&turn, false), "{command}");
    }
    for (command, output, code, good) in [
        ("npm test | tail -8", "# fail 2", Some(0), false),
        ("pytest || true", "ImportError: missing", Some(0), false),
        ("pytest; git diff", "1 failed in 0.2s", Some(0), false),
        ("pytest; git diff", "1 passed in 0.2s", Some(1), true),
        ("pytest | tail -3", "", Some(0), true),
        ("pytest", "", Some(1), false),
    ] {
        let turn = Turn {
            calls: vec![edit(), shell(command, output, code)],
            stopped_by: StopReason::OwnStop,
            ..Turn::default()
        };
        assert_eq!(is_good_turn(&turn, false), good, "{command}");
    }
}
