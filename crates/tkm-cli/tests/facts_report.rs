#![forbid(unsafe_code)]
#![allow(clippy::unwrap_used, clippy::expect_used)]
mod support_report;
use support_report::Fixture;
use tkm_store::{FactRecord, FactState};

fn seed(f: &Fixture, id: &str, active: bool) {
    let store = f.store();
    store
        .insert_candidate_fact(&FactRecord {
            repo_key: f.repo.clone(),
            fact_id: id.into(),
            family: "empty_probe".into(),
            scope: "neutral".into(),
            text_params: serde_json::json!({}),
            state: FactState::Candidate,
            score: 1.0,
            confirmations: 3,
            contradictions: 0,
            fire_count: 3,
            fire_sessions: 4,
            state_changed_at: None,
        })
        .unwrap();
    if active {
        store
            .transition_fact(&f.repo, id, FactState::Active)
            .unwrap();
    }
}

#[test]
fn facts_brief() {
    let f = Fixture::new();
    seed(&f, "f-1", true);
    seed(&f, "f-2", false);
    let out = f.run(&["facts", "--brief"]);
    assert!(out.status.success(), "{out:?}");
    let text = String::from_utf8(out.stdout).unwrap();
    assert!(text.contains("f-1"));
    assert!(!text.contains("f-2"));
}

#[test]
fn facts_all() {
    let f = Fixture::new();
    seed(&f, "f-1", true);
    seed(&f, "f-2", false);
    f.store()
        .transition_fact(&f.repo, "f-1", FactState::Retired)
        .unwrap();
    let out = f.run(&["facts", "--all"]);
    assert!(out.status.success(), "{out:?}");
    let text = String::from_utf8(out.stdout).unwrap();
    assert!(text.contains("f-1") && text.contains("f-2"));
}

#[test]
fn facts_forget_retires() {
    let f = Fixture::new();
    seed(&f, "f-1", true);
    let out = f.run(&["facts", "forget", "f-1"]);
    assert!(out.status.success(), "{out:?}");
    assert_eq!(
        f.store().get_fact(&f.repo, "f-1").unwrap().unwrap().state,
        FactState::Retired
    );
}

#[test]
fn facts_forget_unknown_exits_1() {
    let f = Fixture::new();
    let out = f.run(&["facts", "forget", "missing"]);
    assert_eq!(out.status.code(), Some(1), "{out:?}");
    assert!(!out.stderr.is_empty());
}
