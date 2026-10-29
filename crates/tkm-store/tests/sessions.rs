#![forbid(unsafe_code)]
#![allow(clippy::unwrap_used, clippy::expect_used)]
use serde_json::json;
use std::path::PathBuf;
use tkm_core::model::{Host, RepoKey, Timestamp, Usage};
use tkm_store::{LedgerRecord, NewEvent, NewSession, Repo, SessionKey, Store, TurnRecord};

fn setup() -> (Store, NewSession) {
    let store = Store::open_in_memory().unwrap();
    let repo_key = RepoKey("example/repo".into());
    store
        .upsert_repo(&Repo {
            repo_key: repo_key.clone(),
            display_name: "repo".into(),
            first_seen: Timestamp(1),
            last_seen: Timestamp(1),
            delegated_runs: 0,
        })
        .unwrap();
    (
        store,
        NewSession {
            key: SessionKey {
                host: Host::Codex,
                agent_home: "home".into(),
                session_id: "session".into(),
            },
            repo_key,
            cwd: PathBuf::from("/checkout"),
            started: Timestamp(10),
            ended: None,
            fold_status: "pending".into(),
            shadow: false,
            integration_level: "full".into(),
        },
    )
}
#[test]
fn roundtrip_sessions() {
    let (store, session) = setup();
    let id = store.upsert_session(&session).unwrap();
    let row = store.get_session(id).unwrap().unwrap();
    assert_eq!(row.key, session.key);
    assert_eq!(row.repo_key, session.repo_key);
    assert_eq!(row.cwd, session.cwd);
    assert_eq!(row.started, session.started);
    assert_eq!(store.find_session(&session.key).unwrap(), Some(row));
    store.end_session(id, Timestamp(30)).unwrap();
    store.set_session_fold_status(id, "done").unwrap();
    store.set_session_shadow(id, true).unwrap();
    store.set_integration_level(id, "minimal").unwrap();
    let row = store.get_session(id).unwrap().unwrap();
    assert_eq!(row.ended, Some(Timestamp(30)));
    assert_eq!(row.fold_status, "done");
    assert!(row.shadow);
    assert_eq!(row.integration_level, "minimal");
    let mut second = session.clone();
    second.key.session_id = "second".into();
    second.started = Timestamp(20);
    let second_id = store.upsert_session(&second).unwrap();
    let rows = store.list_sessions_by_repo(&session.repo_key, 1).unwrap();
    assert_eq!(rows[0].id, second_id);
    assert_eq!(store.get_session(-1).unwrap(), None);
}
#[test]
fn upsert_session_is_idempotent_by_key() {
    let (store, mut session) = setup();
    let id = store.upsert_session(&session).unwrap();
    session.started = Timestamp(99);
    session.cwd = PathBuf::from("/new");
    session.shadow = true;
    assert_eq!(store.upsert_session(&session).unwrap(), id);
    let row = store.get_session(id).unwrap().unwrap();
    assert_eq!(row.started, Timestamp(10));
    assert_eq!(row.cwd, session.cwd);
    assert!(row.shadow);
    assert_eq!(
        store
            .list_sessions_by_repo(&session.repo_key, 10)
            .unwrap()
            .len(),
        1
    );
}
#[test]
fn roundtrip_turns() {
    let (store, session) = setup();
    let id = store.upsert_session(&session).unwrap();
    let mut turn = TurnRecord {
        session_id: id,
        turn_idx: 2,
        calls: 3,
        ended_well: false,
        escalated: true,
        digest_hash: Some("abc".into()),
    };
    store.upsert_turn(&turn).unwrap();
    turn.calls = 5;
    store.upsert_turn(&turn).unwrap();
    let mut earlier = turn.clone();
    earlier.turn_idx = 1;
    earlier.digest_hash = None;
    store.upsert_turn(&earlier).unwrap();
    assert_eq!(store.list_turns(id).unwrap(), vec![earlier, turn]);
}
#[test]
fn roundtrip_events() {
    let (store, session) = setup();
    let id = store.upsert_session(&session).unwrap();
    let event = NewEvent {
        kind: "redirect".into(),
        session_id: id,
        inputs: json!({"nested": [1, true]}),
        evidence_ids: vec![3, 7],
        explanation_template_id: "redirect".into(),
        params: json!({"reason": "loop"}),
    };
    let first = store.insert_event(&event).unwrap();
    let second = store.insert_event(&event).unwrap();
    let row = store.get_event(first).unwrap().unwrap();
    assert_eq!(row.kind, event.kind);
    assert_eq!(row.session_id, id);
    assert_eq!(row.inputs, event.inputs);
    assert_eq!(row.evidence_ids, event.evidence_ids);
    assert_eq!(row.explanation_template_id, event.explanation_template_id);
    assert_eq!(row.params, event.params);
    assert_eq!(store.list_events_by_session(id).unwrap().len(), 2);
    assert_eq!(store.list_recent_events(1).unwrap()[0].id, second);
    assert_eq!(store.get_event(-1).unwrap(), None);
}
#[test]
fn roundtrip_ledger() {
    let (store, session) = setup();
    let id = store.upsert_session(&session).unwrap();
    let mut ledger = LedgerRecord {
        session_id: id,
        usage: Usage {
            input: 10,
            output: 20,
            cache_read: 30,
            cache_write: 40,
        },
        priced_cost: Some(1.5),
        overhead_tokens: 7,
        overhead_cost: Some(0.25),
    };
    store.upsert_ledger(&ledger).unwrap();
    assert_eq!(store.get_ledger(id).unwrap(), Some(ledger.clone()));
    ledger.usage.input = 99;
    store.upsert_ledger(&ledger).unwrap();
    assert_eq!(store.get_ledger(id).unwrap(), Some(ledger));
    assert_eq!(store.get_ledger(-1).unwrap(), None);
}
#[test]
fn ledger_rejects_u64_overflow() {
    let (store, session) = setup();
    let id = store.upsert_session(&session).unwrap();
    let mut ledger = LedgerRecord {
        session_id: id,
        usage: Usage::default(),
        priced_cost: Some(0.0),
        overhead_tokens: 0,
        overhead_cost: Some(0.0),
    };
    for field in 0..5 {
        ledger.usage = Usage::default();
        ledger.overhead_tokens = 0;
        match field {
            0 => ledger.usage.input = u64::MAX,
            1 => ledger.usage.output = u64::MAX,
            2 => ledger.usage.cache_read = u64::MAX,
            3 => ledger.usage.cache_write = u64::MAX,
            _ => ledger.overhead_tokens = u64::MAX,
        }
        assert!(store.upsert_ledger(&ledger).is_err());
    }
    assert_eq!(store.get_ledger(id).unwrap(), None);
}

#[test]
fn skill_evidence_is_persistent_per_session_and_replaceable() {
    let (store, session) = setup();
    let id = store.upsert_session(&session).unwrap();
    let mut report = tkm_core::prune::skills::SkillReport::default();
    report.skills.insert(
        "review".into(),
        tkm_core::prune::skills::SkillUsage {
            loaded_sessions: 1,
            invocations: 2,
        },
    );
    store.replace_skill_evidence(id, &report).unwrap();
    store.replace_skill_evidence(id, &report).unwrap();
    assert_eq!(store.skill_evidence(&session.repo_key).unwrap(), report);
    store
        .replace_skill_evidence(id, &Default::default())
        .unwrap();
    assert!(
        store
            .skill_evidence(&session.repo_key)
            .unwrap()
            .skills
            .is_empty()
    );
}

#[test]
fn unknown_session_cost_round_trips_as_null() {
    let (store, session) = setup();
    let id = store.upsert_session(&session).unwrap();
    let ledger = LedgerRecord {
        session_id: id,
        usage: Usage::default(),
        priced_cost: None,
        overhead_tokens: 0,
        overhead_cost: Some(0.0),
    };
    store.upsert_ledger(&ledger).unwrap();
    let stored = store.get_ledger(id).unwrap().unwrap();
    assert_eq!(stored.priced_cost, None);
    assert!(serde_json::to_value(stored).unwrap()["priced_cost"].is_null());
}
