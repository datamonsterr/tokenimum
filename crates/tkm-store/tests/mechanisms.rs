#![forbid(unsafe_code)]
#![allow(clippy::unwrap_used, clippy::expect_used)]
use std::path::PathBuf;
use tkm_core::model::{Host, RepoKey, Timestamp};
use tkm_store::{DenylistEntry, FactRecord, FactState, Repo, Store, StoreError, ToolEvidence};

fn setup() -> (Store, RepoKey) {
    let store = Store::open_in_memory().unwrap();
    let key = RepoKey("example/repo".into());
    store
        .upsert_repo(&Repo {
            repo_key: key.clone(),
            display_name: "example".into(),
            first_seen: Timestamp(1),
            last_seen: Timestamp(2),
            delegated_runs: 0,
        })
        .unwrap();
    (store, key)
}
fn fact(key: &RepoKey) -> FactRecord {
    FactRecord {
        repo_key: key.clone(),
        fact_id: "f".into(),
        family: "form".into(),
        scope: "neutral".into(),
        text_params: serde_json::json!({"form":"cargo test"}),
        state: FactState::Candidate,
        score: 0.5,
        confirmations: 1,
        contradictions: 2,
        fire_count: 3,
        fire_sessions: 4,
    }
}
#[test]
fn roundtrip_facts() {
    let (store, key) = setup();
    let mut value = fact(&key);
    value.state = FactState::Retired;
    store.insert_candidate_fact(&value).unwrap();
    value.state = FactState::Candidate;
    assert_eq!(store.get_fact(&key, "f").unwrap(), Some(value.clone()));
    assert!(store.insert_candidate_fact(&value).is_err());
    assert_eq!(store.list_facts(&key, None).unwrap(), vec![value.clone()]);
    assert_eq!(
        store
            .list_facts(&key, Some(FactState::Active))
            .unwrap()
            .len(),
        0
    );
    store.update_fact_stats(&key, "f", 0.8, 4, 5, 6, 7).unwrap();
    value.score = 0.8;
    value.confirmations = 4;
    value.contradictions = 5;
    value.fire_count = 6;
    value.fire_sessions = 7;
    assert_eq!(store.get_fact(&key, "f").unwrap(), Some(value));
}
#[test]
fn roundtrip_tool_evidence() {
    let (store, key) = setup();
    store
        .record_tool_unused_prompt(&key, Host::Claude, "read")
        .unwrap();
    store
        .record_tool_unused_session(&key, Host::Claude, "read")
        .unwrap();
    let value = ToolEvidence {
        repo_key: key.clone(),
        host: Host::Claude,
        tool: "read".into(),
        prompts_unused: 1,
        sessions_unused: 1,
        last_used: None,
    };
    assert_eq!(
        store.get_tool_evidence(&key, Host::Claude, "read").unwrap(),
        Some(value.clone())
    );
    assert_eq!(
        store.list_tool_evidence(&key, Host::Claude).unwrap(),
        vec![value]
    );
    assert_eq!(
        store.list_tool_evidence(&key, Host::Codex).unwrap().len(),
        0
    );
}
#[test]
fn roundtrip_denylist_applied() {
    let (store, _) = setup();
    let mut value = DenylistEntry {
        checkout_path: PathBuf::from("/tmp/checkout"),
        host: Host::Claude,
        tool: "read".into(),
        written_at: Timestamp(42),
        restored: false,
    };
    store.record_denylist_write(&value).unwrap();
    assert_eq!(
        store.list_denylist(&value.checkout_path).unwrap(),
        vec![value.clone()]
    );
    store
        .mark_denylist_restored(&value.checkout_path, value.host, &value.tool)
        .unwrap();
    value.restored = true;
    assert_eq!(
        store.list_denylist(&value.checkout_path).unwrap(),
        vec![value]
    );
}
#[test]
fn roundtrip_envelopes() {
    let (store, key) = setup();
    assert_eq!(store.envelope_samples(&key, Host::Claude).unwrap().len(), 0);
    store.push_envelope_sample(&key, Host::Claude, 7).unwrap();
    store.push_envelope_sample(&key, Host::Claude, 9).unwrap();
    assert_eq!(
        store.envelope_samples(&key, Host::Claude).unwrap(),
        vec![7, 9]
    );
    assert_eq!(store.envelope_samples(&key, Host::Codex).unwrap().len(), 0);
}
#[test]
fn fact_transitions_allowed() {
    for (from, to) in [
        (FactState::Candidate, FactState::Active),
        (FactState::Candidate, FactState::Retired),
        (FactState::Active, FactState::Retired),
        (FactState::Retired, FactState::Candidate),
    ] {
        let (store, key) = setup();
        store.insert_candidate_fact(&fact(&key)).unwrap();
        if from == FactState::Active {
            store.transition_fact(&key, "f", FactState::Active).unwrap();
        }
        if from == FactState::Retired {
            store
                .transition_fact(&key, "f", FactState::Retired)
                .unwrap();
        }
        store.transition_fact(&key, "f", to).unwrap();
        assert_eq!(store.get_fact(&key, "f").unwrap().unwrap().state, to);
    }
}
#[test]
fn fact_transitions_disallowed_leave_state_unchanged() {
    for (from, to) in [
        (FactState::Candidate, FactState::Candidate),
        (FactState::Active, FactState::Candidate),
        (FactState::Active, FactState::Active),
        (FactState::Retired, FactState::Active),
        (FactState::Retired, FactState::Retired),
    ] {
        let (store, key) = setup();
        store.insert_candidate_fact(&fact(&key)).unwrap();
        if from != FactState::Candidate {
            store.transition_fact(&key, "f", from).unwrap();
        }
        let before = store.get_fact(&key, "f").unwrap();
        assert!(matches!(
            store.transition_fact(&key, "f", to),
            Err(StoreError::InvalidTransition)
        ));
        assert_eq!(store.get_fact(&key, "f").unwrap(), before);
    }
}
#[test]
fn envelope_ring_keeps_last_500_in_order() {
    let (store, key) = setup();
    for n in 0..520 {
        store.push_envelope_sample(&key, Host::Claude, n).unwrap();
    }
    assert_eq!(
        store.envelope_samples(&key, Host::Claude).unwrap(),
        (20..520).collect::<Vec<u32>>()
    );
}
#[test]
fn tool_used_resets_unused_counters() {
    let (store, key) = setup();
    store
        .record_tool_unused_prompt(&key, Host::Claude, "read")
        .unwrap();
    store
        .record_tool_unused_session(&key, Host::Claude, "read")
        .unwrap();
    store
        .record_tool_used(&key, Host::Claude, "read", Timestamp(123))
        .unwrap();
    let value = store
        .get_tool_evidence(&key, Host::Claude, "read")
        .unwrap()
        .unwrap();
    assert_eq!(
        (value.prompts_unused, value.sessions_unused, value.last_used),
        (0, 0, Some(Timestamp(123)))
    );
}
