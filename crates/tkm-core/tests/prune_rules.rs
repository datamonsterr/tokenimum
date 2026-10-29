#![forbid(unsafe_code)]
use proptest::prelude::*;
use tkm_core::{
    model::{Call, Host, Session, Timestamp, ToolDef, ToolKind, Turn},
    prune::*,
};

fn session(name: &str, prompts: usize, used: bool) -> Session {
    let mut s = Session::new(Host::Claude);
    s.tool_catalog.push(ToolDef {
        name: name.into(),
        kind: ToolKind::Mcp,
        ..ToolDef::default()
    });
    s.turns = (0..prompts)
        .map(|i| Turn {
            calls: if used && i == 0 {
                vec![Call {
                    tool: name.into(),
                    ..Call::default()
                }]
            } else {
                vec![]
            },
            ..Turn::default()
        })
        .collect();
    s
}
#[test]
fn thresholds_and_reset() {
    let cfg = PruneConfig::default();
    for (prompts, sessions, expected) in [
        (9, 2, false),
        (10, 1, false),
        (10, 2, true),
        (0, 5, true),
        (0, 4, false),
    ] {
        assert_eq!(
            eligible(
                &ToolEvidence {
                    prompts,
                    sessions,
                    ..ToolEvidence::default()
                },
                &cfg
            ),
            expected
        );
    }
    let mut state = PruneEvidence::default();
    fold(&mut state, &session("mcp__x", 5, false));
    fold(&mut state, &session("mcp__x", 5, false));
    assert!(
        decide(&state, Host::Claude, &cfg, None, Timestamp(0))
            .candidates
            .contains(&"mcp__x".into())
    );
    fold(&mut state, &session("mcp__x", 1, true));
    assert!(
        !decide(&state, Host::Claude, &cfg, None, Timestamp(0))
            .candidates
            .contains(&"mcp__x".into())
    );
}
#[test]
fn quiet_window_and_cap() {
    let c = PruneConfig::default();
    for (activity, ready, now, expected) in [
        (0, 0, 3_599_999, false),
        (0, 0, 3_600_000, true),
        (86_399_999, 0, 86_400_000, true),
        (100, 100, 0, false),
    ] {
        assert_eq!(
            write_allowed(Timestamp(activity), Timestamp(ready), Timestamp(now), &c),
            expected
        );
    }
}
#[test]
fn modes_hosts_and_delegation() {
    for host in [Host::Claude, Host::Cursor, Host::Opencode, Host::Pi] {
        for mode in [
            PruneMode::Off,
            PruneMode::Report,
            PruneMode::Propose,
            PruneMode::Apply,
        ] {
            let c = PruneConfig {
                mode,
                ..PruneConfig::default()
            };
            assert_eq!(effective_mode(host, &c, None, Timestamp(0)), mode);
        }
    }
    for host in [Host::Codex, Host::Agy, Host::Kimi, Host::Deepseek] {
        let c = PruneConfig {
            mode: PruneMode::Apply,
            ..PruneConfig::default()
        };
        assert_eq!(
            effective_mode(host, &c, None, Timestamp(0)),
            PruneMode::Report
        );
    }
    let c = PruneConfig {
        mode: PruneMode::Apply,
        superorchestrator_adapter_enabled: true,
        ..PruneConfig::default()
    };
    assert_eq!(
        effective_mode(
            Host::Claude,
            &c,
            Some(Timestamp(0)),
            Timestamp(30 * 86_400_000)
        ),
        PruneMode::Propose
    );
    assert_eq!(
        effective_mode(
            Host::Claude,
            &c,
            Some(Timestamp(0)),
            Timestamp(30 * 86_400_000 + 1)
        ),
        PruneMode::Apply
    );
}
const TE_PROTECTED: &[&str] = &[
    "bash",
    "view",
    "grep",
    "glob",
    "edit",
    "create",
    "str_replace",
    "read_bash",
    "stop_bash",
    "list_bash",
    "read_file",
    "shell",
    "apply_patch",
    "Bash",
    "Edit",
    "Write",
    "Read",
    "Grep",
    "Glob",
    "MultiEdit",
    "LS",
    "NotebookRead",
    "BashOutput",
    "KillShell",
    "KillBash",
    "AskUserQuestion",
    "EnterPlanMode",
    "ExitPlanMode",
    "TodoWrite",
    "TaskCreate",
    "TaskUpdate",
    "TaskGet",
    "TaskList",
    "TaskOutput",
    "TaskStop",
    "Agent",
    "Task",
    "Skill",
    "ToolSearch",
    "SlashCommand",
    "Monitor",
    "WebFetch",
    "WebSearch",
];
proptest! {
    #[test]
    fn no_protected_tool_in_candidates(names in proptest::collection::vec(prop_oneof![proptest::sample::select(TE_PROTECTED).prop_map(str::to_owned),Just("skill".to_string()),Just("task".to_string()),Just("read".to_string()),Just("write".to_string()),Just("mcp__headroom__x".to_string()),Just("mcp__superorchestrator__y".to_string()),Just("custom__x".to_string()),Just("exact".to_string()),"[a-z]{1,12}"], 1..30), used in proptest::collection::vec(any::<bool>(), 0..20), host_index in 0usize..3) {
        let host = [Host::Claude, Host::Opencode, Host::Pi][host_index];
        let c=PruneConfig { protect: vec!["custom__*".into(),"exact".into()],superorchestrator_adapter_enabled:true,..PruneConfig::default() };
        let mut state=PruneEvidence::default();
        for name in names { for flag in &used { let mut s = session(&name, 2,*flag); s.host = host; fold(&mut state,&s); } for _ in 0..5 { let mut s = session(&name,2,false); s.host = host; fold(&mut state,&s); } }
        let d=decide(&state,host,&c,None,Timestamp(0));
        for name in d.candidates {
            let host_protected = match host { Host::Opencode => ["skill","task"].contains(&name.as_str()), Host::Pi => ["read","bash","edit","write"].contains(&name.as_str()), _ => false };
            prop_assert!(!TE_PROTECTED.contains(&name.as_str()) && !host_protected && !name.starts_with("mcp__headroom__") && !name.starts_with("mcp__superorchestrator__") && !name.starts_with("custom__") && name != "exact");
        }
    }
}

#[test]
fn partial_sessions_and_skill_report() {
    use tkm_core::prune::skills::{SkillReport, fold_skills};
    let mut s = session("plugin__unused", 10, false);
    s.tool_catalog.push(ToolDef {
        name: "test-skill".into(),
        kind: ToolKind::Skill,
        ..ToolDef::default()
    });
    s.turns[0].calls.push(Call {
        tool: "Skill".into(),
        args_digest: "{\"skill\":\"test-skill\"}".into(),
        ..Call::default()
    });
    let mut evidence = PruneEvidence::default();
    let mut report = SkillReport::default();
    s.partial = true;
    fold(&mut evidence, &s);
    fold_skills(&mut report, &s);
    assert!(evidence.tools.is_empty());
    assert!(report.skills.is_empty());
    s.partial = false;
    fold(&mut evidence, &s);
    fold_skills(&mut report, &s);
    assert!(!evidence.tools.contains_key("test-skill"));
    assert_eq!(report.skills["test-skill"].loaded_sessions, 1);
    assert_eq!(report.skills["test-skill"].invocations, 1);
    assert!(serde_json::to_string(&report).is_ok());
}

#[test]
fn evidence_isolated_by_repository_and_host() {
    use tkm_core::model::RepoKey;
    let mut state = PruneEvidence::default();
    let first = session("mcp__unused", 5, false);
    fold(&mut state, &first);
    let mut other = first.clone();
    other.repo_key = RepoKey("other".into());
    fold(&mut state, &other);
    other.repo_key = first.repo_key.clone();
    other.host = Host::Pi;
    fold(&mut state, &other);
    assert_eq!(state.tools["mcp__unused"].sessions, 1);
    assert_eq!(state.tools["mcp__unused"].prompts, 5);
}

#[test]
fn protected_sets_cover_reference_and_host_equivalents() {
    let cfg = PruneConfig {
        protect: vec!["private__*".into()],
        superorchestrator_adapter_enabled: true,
        ..PruneConfig::default()
    };
    for name in [
        "Bash",
        "Agent",
        "Skill",
        "ToolSearch",
        "WebFetch",
        "WebSearch",
        "mcp__headroom__x",
        "mcp__superorchestrator__y",
        "private__anything",
    ] {
        assert!(is_protected(name, Host::Claude, &cfg), "{name}");
    }
    for name in ["skill", "task"] {
        assert!(is_protected(name, Host::Opencode, &cfg));
    }
    for name in ["read", "bash", "edit", "write"] {
        assert!(is_protected(name, Host::Pi, &cfg));
    }
}

#[test]
fn used_is_permanently_excluded_and_session_use_resets_streak() {
    for index in 0..3 {
        let mut state = PruneEvidence::default();
        fold(&mut state, &session("mcp__x", 4, false));
        let mut s = session("mcp__x", 3, false);
        s.turns[index].calls.push(Call {
            tool: "mcp__x".into(),
            ..Call::default()
        });
        fold(&mut state, &s);
        assert_eq!(
            (
                state.tools["mcp__x"].prompts,
                state.tools["mcp__x"].sessions
            ),
            (0, 0)
        );
        for _ in 0..10 {
            fold(&mut state, &session("mcp__x", 3, false));
        }
        assert!(
            !decide(
                &state,
                Host::Claude,
                &PruneConfig::default(),
                None,
                Timestamp(0)
            )
            .candidates
            .contains(&"mcp__x".into())
        );
    }
}
#[test]
fn no_prompts_no_evidence() {
    let mut state = PruneEvidence::default();
    for _ in 0..6 {
        fold(&mut state, &session("mcp__x", 0, false));
    }
    assert!(state.tools.is_empty());
}
#[test]
fn report_only_mode_matrix() {
    for host in [
        Host::Claude,
        Host::Cursor,
        Host::Opencode,
        Host::Pi,
        Host::Codex,
        Host::Agy,
        Host::Kimi,
        Host::Deepseek,
    ] {
        for mode in [
            PruneMode::Off,
            PruneMode::Report,
            PruneMode::Propose,
            PruneMode::Apply,
        ] {
            let c = PruneConfig {
                mode,
                ..PruneConfig::default()
            };
            let expected = if matches!(host, Host::Codex | Host::Agy | Host::Kimi | Host::Deepseek)
                && matches!(mode, PruneMode::Propose | PruneMode::Apply)
            {
                PruneMode::Report
            } else {
                mode
            };
            assert_eq!(effective_mode(host, &c, None, Timestamp(0)), expected);
        }
    }
}
#[test]
fn skill_target_is_value_not_argument_key() {
    use tkm_core::prune::skills::{SkillReport, fold_skills};
    let mut s = session("x", 1, false);
    for name in ["skill", "target-skill"] {
        s.tool_catalog.push(ToolDef {
            name: name.into(),
            kind: ToolKind::Skill,
            ..ToolDef::default()
        });
    }
    s.turns[0].calls.push(Call {
        tool: "Skill".into(),
        args_digest: r#"{"skill":"target-skill"}"#.into(),
        ..Call::default()
    });
    let mut report = SkillReport::default();
    fold_skills(&mut report, &s);
    assert_eq!(report.skills["skill"].invocations, 0);
    assert_eq!(report.skills["target-skill"].invocations, 1);
}
#[test]
fn deny_ownership_keep_and_checkout_isolation() {
    use std::collections::BTreeSet;
    use tkm_core::prune::denylist::{DenyState, apply, restore, set_kept};
    let set = |xs: &[&str]| -> BTreeSet<String> { xs.iter().map(|s| (*s).to_owned()).collect() };
    let mut state = DenyState::default();
    let current = set(&["foreign"]);
    let requested = set(&["foreign", "owned"]);
    let delta = apply(&mut state, "one", &current, &requested);
    assert_eq!(delta.add, set(&["owned"]));
    let current = set(&["foreign", "owned"]);
    assert!(
        apply(&mut state, "one", &current, &requested)
            .add
            .is_empty()
    );
    assert!(
        restore(&mut state, "two", &current, &requested)
            .remove
            .is_empty()
    );
    assert_eq!(
        restore(&mut state, "one", &current, &requested).remove,
        set(&["owned"])
    );
    assert!(
        apply(&mut state, "one", &set(&["foreign"]), &requested)
            .add
            .is_empty()
    );
    set_kept(&mut state, "one", &set(&[]), &set(&["owned"]));
    assert_eq!(
        apply(&mut state, "one", &set(&["foreign"]), &requested).add,
        set(&["owned"])
    );
}

#[test]
fn failed_deny_apply_and_restore_are_retryable() {
    use std::collections::BTreeSet;
    use tkm_core::prune::denylist::{DenyState, apply, restore};
    let requested: BTreeSet<_> = ["tool".to_owned()].into_iter().collect();
    let empty = BTreeSet::new();
    let mut state = DenyState::default();
    let original = state.clone();
    let first = apply(&mut state, "checkout", &empty, &requested);
    assert_eq!(state, original, "planning must not commit on failed apply");
    let retry = apply(&mut state, "checkout", &empty, &requested);
    assert_eq!(first, retry);
    assert_eq!(retry.add, requested);
    // Model a successfully applied addition, then a failed restore.
    state.applied.insert("checkout".into(), requested.clone());
    let original = state.clone();
    let first = restore(&mut state, "checkout", &requested, &requested);
    assert_eq!(
        state, original,
        "planning must not commit on failed restore"
    );
    let retry = restore(&mut state, "checkout", &requested, &requested);
    assert_eq!(first, retry);
    assert_eq!(retry.remove, requested);
}

#[test]
fn skill_json_extra_fields_incomplete_catalog_and_ambiguity() {
    use tkm_core::prune::skills::{SkillReport, fold_skills};
    let mut s = session("x", 1, false);
    for digest in [
        r#"{"skill":"review","args":"focused"}"#,
        r#"{"skill":"review","skill":"other"}"#,
        r#"{"skill":42}"#,
        r#"{"skill":"review"} trailing"#,
    ] {
        s.turns[0].calls.push(Call {
            tool: "Skill".into(),
            args_digest: digest.into(),
            ..Call::default()
        });
    }
    let mut report = SkillReport::default();
    fold_skills(&mut report, &s);
    assert_eq!(report.skills["review"].loaded_sessions, 0);
    assert_eq!(report.skills["review"].invocations, 1);
    assert!(!report.skills.contains_key("other"));
}
