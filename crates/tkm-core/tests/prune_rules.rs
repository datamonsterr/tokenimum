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
proptest! {
    #[test]
    fn no_protected_tool_in_candidates(names in proptest::collection::vec(prop_oneof![Just("Bash".to_string()),Just("Skill".to_string()),Just("mcp__headroom__x".to_string()),Just("mcp__superorchestrator__y".to_string()),Just("custom__x".to_string()),"[a-z]{1,12}"], 1..30), used in proptest::collection::vec(any::<bool>(), 0..20)) {
        let c=PruneConfig { protect: vec!["custom__*".into()],superorchestrator_adapter_enabled:true,..PruneConfig::default() };
        let mut state=PruneEvidence::default();
        for name in names { for flag in &used { fold(&mut state,&session(&name, 2,*flag)); } for _ in 0..5 { fold(&mut state,&session(&name,2,false)); } }
        let d=decide(&state,Host::Claude,&c,None,Timestamp(0));
        for name in d.candidates { prop_assert!(!is_protected(&name,Host::Claude,&c)); }
        prop_assert!(is_protected("Bash",Host::Claude,&c));
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
