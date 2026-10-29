#![forbid(unsafe_code)]
#![allow(clippy::unwrap_used, clippy::expect_used)]
mod support_report;
use support_report::Fixture;
use tkm_core::model::{Host, Timestamp};
use tkm_store::{NewEvent, NewSession, SessionKey};

fn seed(f: &Fixture, session: &str, detail: &str) -> i64 {
    let store = f.store();
    let session_id = store
        .upsert_session(&NewSession {
            key: SessionKey {
                host: Host::Claude,
                agent_home: "/fixture".into(),
                session_id: session.into(),
            },
            repo_key: f.repo.clone(),
            cwd: f.home.path().to_path_buf(),
            started: Timestamp(1),
            ended: None,
            fold_status: "open".into(),
            shadow: false,
            integration_level: "hooks".into(),
        })
        .unwrap();
    store.insert_event(&NewEvent { kind: "loop.redirect".into(), session_id,
        inputs: serde_json::json!({}), evidence_ids: vec![], explanation_template_id: "redirect".into(),
        params: serde_json::json!({"event":"fixture", "detail":detail, "why":"evidence", "model_checks":"none", "cost":"$0", "undo":"none"}) }).unwrap()
}

#[test]
fn why_last() {
    let f = Fixture::new();
    seed(&f, "one", "first");
    seed(&f, "two", "second");
    let out = f.run(&["why", "last"]);
    assert!(out.status.success(), "{out:?}");
    assert!(String::from_utf8(out.stdout).unwrap().contains("second"));
}
#[test]
fn why_session_filter() {
    let f = Fixture::new();
    seed(&f, "one", "first");
    seed(&f, "two", "second");
    let out = f.run(&["why", "last", "--session", "one"]);
    assert!(out.status.success(), "{out:?}");
    assert!(String::from_utf8(out.stdout).unwrap().contains("first"));
}
#[test]
fn why_unknown_id_exits_1() {
    let f = Fixture::new();
    let out = f.run(&["why", "99999"]);
    assert_eq!(out.status.code(), Some(1), "{out:?}");
    assert!(!out.stderr.is_empty());
}

#[test]
fn why_last_ignores_newer_observations() {
    let f = Fixture::new();
    let id = seed(&f, "one", "action detail");
    let store = f.store();
    let mut event = store.get_event(id).unwrap().unwrap();
    event.kind = "hook.seen".into();
    store
        .insert_event(&NewEvent {
            kind: event.kind,
            session_id: event.session_id,
            inputs: serde_json::json!({}),
            evidence_ids: vec![],
            explanation_template_id: "observation".into(),
            params: serde_json::json!({}),
        })
        .unwrap();
    let out = f.run(&["why", "last"]);
    assert!(out.status.success(), "{out:?}");
    let text = String::from_utf8(out.stdout).unwrap();
    assert!(text.contains("action detail") && text.contains("unknown time"));
}
