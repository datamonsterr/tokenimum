#![forbid(unsafe_code)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::pedantic)]
mod support_report;
use support_report::Fixture;
use tkm_core::model::{RepoKey, Timestamp};
use tkm_store::Repo;

#[test]
fn status_daemon_down_uses_db() {
    let f = Fixture::new();
    let out = f.run(&["status"]);
    assert!(out.status.success(), "{out:?}");
    let text = String::from_utf8(out.stdout).unwrap();
    assert!(text.contains("down (read-only DB)"));
    assert!(text.contains(&f.repo.0));
}
#[test]
fn status_all_lists_repos() {
    let f = Fixture::new();
    f.store()
        .upsert_repo(&Repo {
            repo_key: RepoKey("fixture/other".into()),
            display_name: "other".into(),
            first_seen: Timestamp(1),
            last_seen: Timestamp(1),
            delegated_runs: 0,
        })
        .unwrap();
    let out = f.run(&["status", "--all"]);
    assert!(out.status.success(), "{out:?}");
    let text = String::from_utf8(out.stdout).unwrap();
    assert!(text.contains(&f.repo.0));
    assert!(text.contains("fixture/other"));
}

fn report_at(f: &Fixture, now: i64) -> serde_json::Value {
    let paths = tkm_config::paths::Paths::from_dirs(
        f.home.path(),
        Some(&f.home.path().join("data")),
        Some(&f.home.path().join("config")),
        Some(&f.home.path().join("run")),
    );
    let repo = tkm_config::repo::repo_identity(f.home.path(), "", Some(f.home.path()));
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let mut ctx = tkm_cli::ctx::Ctx::new(paths, f.home.path().into(), repo, &mut out, &mut err);
    assert_eq!(
        tkm_cli::cmd::status::run_at(
            &mut ctx,
            &tkm_cli::cli::StatusArgs {
                all: false,
                json: true
            },
            now
        )
        .unwrap(),
        std::process::ExitCode::SUCCESS
    );
    assert!(err.is_empty());
    serde_json::from_slice(&out).unwrap()
}
#[test]
fn status_windows_exclude_unknown_old_and_future_times_and_limit_actions() {
    use tkm_core::model::Host;
    use tkm_store::{FactRecord, FactState, NewEvent, NewSession, SessionKey};
    let f = Fixture::new();
    let store = f.store();
    let now = 1_793_715_780_000;
    for (id, time) in [
        ("legacy", None),
        ("old", Some(now - 8 * 86_400_000)),
        ("recent", Some(now - 1)),
        ("future", Some(now + 1)),
    ] {
        store
            .insert_candidate_fact(&FactRecord {
                repo_key: f.repo.clone(),
                fact_id: id.into(),
                family: "empty_probe".into(),
                scope: "neutral".into(),
                text_params: serde_json::json!({}),
                state: FactState::Candidate,
                score: 0.0,
                confirmations: 0,
                contradictions: 0,
                fire_count: 0,
                fire_sessions: 0,
                state_changed_at: None,
            })
            .unwrap();
        if let Some(at) = time {
            store
                .transition_fact_at(&f.repo, id, FactState::Retired, Timestamp(at))
                .unwrap();
        } else {
            store
                .transition_fact(&f.repo, id, FactState::Retired)
                .unwrap();
        }
    }
    let sid = store
        .upsert_session(&NewSession {
            key: SessionKey {
                host: Host::Claude,
                agent_home: "/fixture".into(),
                session_id: "s".into(),
            },
            repo_key: f.repo.clone(),
            cwd: f.home.path().into(),
            started: Timestamp(now - 1_000),
            ended: None,
            fold_status: "open".into(),
            shadow: false,
            integration_level: "hooks".into(),
        })
        .unwrap();
    let mut event = NewEvent {
        kind: "loop.redirect".into(),
        session_id: sid,
        inputs: serde_json::json!({}),
        evidence_ids: vec![],
        explanation_template_id: "redirect".into(),
        params: serde_json::json!({}),
    };
    for _ in 0..6 {
        store.insert_event_at(&event, Timestamp(now)).unwrap();
    }
    store.insert_event(&event).unwrap();
    event.kind = "hook.seen".into();
    event.explanation_template_id = "observation".into();
    event.inputs = serde_json::json!({"native":"SessionStart","event":"session_start"});
    store.insert_event_at(&event, Timestamp(now)).unwrap();
    let report = report_at(&f, now);
    let repo = &report["repositories"][0];
    assert_eq!(repo["facts"]["retired_last_7_days"], 1);
    assert_eq!(repo["loop_guard"]["redirects_last_7_days"], 6);
    assert_eq!(repo["last_actions"].as_array().unwrap().len(), 5);
    assert_eq!(repo["last_actions"][0]["time"], "unknown time");
    assert_eq!(repo["harnesses"][0]["last_event"], "2026-11-03 14:23 UTC");
    assert!(
        !repo["harnesses"][0]["events_missing"]
            .as_array()
            .unwrap()
            .contains(&serde_json::json!("SessionStart"))
    );
}

#[test]
fn status_spend_is_global_and_uses_the_utc_day() {
    let f = Fixture::new();
    let store = f.store();
    let now = 1_793_715_780_000;
    let day = now / 86_400_000 * 86_400_000;
    let decision = tkm_store::NewDecision {
        model: "fixture".into(),
        purpose: "relevance".into(),
        questions: serde_json::json!({}),
        answers: serde_json::json!({}),
        cost_usd: Some(0.125),
        latency_ms: 1,
        shadow: false,
    };
    for time in [None, Some(day - 1), Some(day), Some(now), Some(now + 1)] {
        store
            .insert_decision_with_context(&decision, time.map(Timestamp), None, None, None)
            .unwrap();
    }
    let report = report_at(&f, now);
    assert_eq!(
        report["repositories"][0]["spend"]["decisions_today_usd"],
        0.25
    );
    assert_eq!(report["repositories"][0]["spend"]["scope"], "global");
}

#[cfg(unix)]
#[test]
fn status_uses_daemon_metadata_and_legacy_unknown() {
    use std::io::Write;
    use std::os::unix::net::UnixListener;
    use tkm_proto::monitor::{MONITOR_MAX_FRAME, MONITOR_SOCKET_NAME, MonitorMessage};
    let f = Fixture::new();
    let socket = f
        .home
        .path()
        .join("run/tokenimum")
        .join(MONITOR_SOCKET_NAME);
    std::fs::create_dir_all(socket.parent().unwrap()).unwrap();
    let now = 1_793_715_780_000;
    for modern in [true, false] {
        if socket.exists() {
            std::fs::remove_file(&socket).unwrap();
        }
        let listener = UnixListener::bind(&socket).unwrap();
        let peer = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let hello = MonitorMessage::Hello {
                version: tkm_proto::PROTOCOL_VERSION,
                daemon_version: modern.then(|| "0.2.0".into()),
                started_at_ms: modern.then_some(now - 30_000),
            };
            stream
                .write_all(&tkm_proto::encode_frame(&hello, MONITOR_MAX_FRAME).unwrap())
                .unwrap();
        });
        let report = report_at(&f, now);
        peer.join().unwrap();
        assert_eq!(report["daemon"]["state"], "running");
        assert_eq!(
            report["daemon"]["version"],
            if modern { "0.2.0" } else { "unknown" }
        );
        assert_eq!(
            report["daemon"]["uptime"],
            if modern { "30s" } else { "unknown" }
        );
    }
}

#[test]
fn status_reports_envelope_pending_tools_and_savings_samples() {
    use tkm_core::model::{Host, Usage};
    use tkm_store::{LedgerRecord, NewEvent, NewSession, SessionKey};
    let f = Fixture::new();
    let store = f.store();
    let mut action_session = 0;
    for (index, cost) in [10.0, 12.0, 6.0, 8.0].into_iter().enumerate() {
        let sid = store
            .upsert_session(&NewSession {
                key: SessionKey {
                    host: Host::Claude,
                    agent_home: "/fixture".into(),
                    session_id: format!("s-{index}"),
                },
                repo_key: f.repo.clone(),
                cwd: f.home.path().into(),
                started: Timestamp(i64::try_from(index + 1).unwrap() * 1000),
                ended: Some(Timestamp(5000)),
                fold_status: "folded".into(),
                shadow: index < 2,
                integration_level: "hooks".into(),
            })
            .unwrap();
        if index == 2 {
            action_session = sid;
        }
        store
            .upsert_ledger(&LedgerRecord {
                session_id: sid,
                usage: Usage::default(),
                priced_cost: Some(cost),
                overhead_tokens: 0,
                overhead_cost: Some(0.0),
            })
            .unwrap();
    }
    store
        .insert_event_at(
            &NewEvent {
                kind: "loop.redirect".into(),
                session_id: action_session,
                inputs: serde_json::json!({"shadow":false}),
                evidence_ids: vec![],
                explanation_template_id: "redirect".into(),
                params: serde_json::json!({}),
            },
            Timestamp(3000),
        )
        .unwrap();
    for _ in 0..5 {
        store
            .push_envelope_sample(&f.repo, Host::Claude, 4)
            .unwrap();
    }
    for tool in ["unused_plugin", "Read"] {
        for _ in 0..10 {
            store
                .record_tool_unused_prompt(&f.repo, Host::Claude, tool)
                .unwrap();
        }
        for _ in 0..2 {
            store
                .record_tool_unused_session(&f.repo, Host::Claude, tool)
                .unwrap();
        }
    }
    let json = report_at(&f, 6000);
    let repo = &json["repositories"][0];
    assert_eq!(repo["loop_guard"]["envelopes"][0]["calls"], 5.0);
    assert_eq!(repo["pruning"]["pending_proposals"], 1);
    let savings = &repo["savings"][0]["estimate"];
    assert_eq!(savings["baseline_count"], 2);
    assert_eq!(savings["treatment_count"], 2);
    assert_eq!(savings["difference"], 4.0);
    assert!(savings["band"].is_object());
}
