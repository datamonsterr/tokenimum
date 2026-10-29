#![forbid(unsafe_code)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::pedantic)]
use tkm_core::model::{Host, RepoKey, Timestamp};
use tkm_store::{
    FactRecord, FactState, NewDecision, NewEvent, NewSession, Repo, SessionKey, Store,
};

#[test]
fn report_context_round_trips_and_legacy_writes_remain_null() {
    let store = Store::open_in_memory().unwrap();
    let repo = RepoKey("fixture/report".into());
    store
        .upsert_repo(&Repo {
            repo_key: repo.clone(),
            display_name: "report".into(),
            first_seen: Timestamp(1),
            last_seen: Timestamp(1),
            delegated_runs: 0,
        })
        .unwrap();
    let sid = store
        .upsert_session(&NewSession {
            key: SessionKey {
                host: Host::Claude,
                agent_home: "/fixture".into(),
                session_id: "s".into(),
            },
            repo_key: repo.clone(),
            cwd: "/fixture".into(),
            started: Timestamp(1),
            ended: None,
            fold_status: "open".into(),
            shadow: false,
            integration_level: "hooks".into(),
        })
        .unwrap();
    let event = NewEvent {
        kind: "redirect".into(),
        session_id: sid,
        inputs: serde_json::json!({}),
        evidence_ids: vec![7],
        explanation_template_id: "redirect".into(),
        params: serde_json::json!({}),
    };
    let legacy = store.insert_event(&event).unwrap();
    let timed = store.insert_event_at(&event, Timestamp(42)).unwrap();
    assert_eq!(store.get_event(legacy).unwrap().unwrap().created_at, None);
    assert_eq!(
        store.get_event(timed).unwrap().unwrap().created_at,
        Some(Timestamp(42))
    );
    let decision = NewDecision {
        model: "fixture".into(),
        purpose: "relevance".into(),
        questions: serde_json::json!({}),
        answers: serde_json::json!({}),
        cost_usd: 0.000006,
        latency_ms: 1,
        shadow: false,
    };
    let legacy = store.insert_decision(&decision).unwrap();
    let contextual = store
        .insert_decision_with_context(
            &decision,
            Some(Timestamp(42)),
            Some(&repo),
            Some(sid),
            Some("[redacted:openrouter]"),
        )
        .unwrap();
    assert_eq!(
        store.get_decision(legacy).unwrap().unwrap().created_at,
        None
    );
    let row = store.get_decision(contextual).unwrap().unwrap();
    assert_eq!(row.repo_key, Some(repo.clone()));
    assert_eq!(row.session_id, Some(sid));
    assert_eq!(row.created_at, Some(Timestamp(42)));
    assert_eq!(row.digest.as_deref(), Some("[redacted:openrouter]"));
    store
        .insert_candidate_fact(&FactRecord {
            repo_key: repo.clone(),
            fact_id: "f-1".into(),
            family: "empty_probe".into(),
            scope: "neutral".into(),
            text_params: serde_json::json!({}),
            state: FactState::Candidate,
            score: 0.0,
            confirmations: 0,
            contradictions: 0,
            fire_count: 0,
            fire_sessions: 0,
            state_changed_at: Some(Timestamp(5)),
        })
        .unwrap();
    assert_eq!(
        store
            .get_fact(&repo, "f-1")
            .unwrap()
            .unwrap()
            .state_changed_at,
        None
    );
    store
        .transition_fact_at(&repo, "f-1", FactState::Active, Timestamp(42))
        .unwrap();
    assert_eq!(
        store
            .get_fact(&repo, "f-1")
            .unwrap()
            .unwrap()
            .state_changed_at,
        Some(Timestamp(42))
    );
    store
        .transition_fact(&repo, "f-1", FactState::Retired)
        .unwrap();
    assert_eq!(
        store
            .get_fact(&repo, "f-1")
            .unwrap()
            .unwrap()
            .state_changed_at,
        None
    );
}
