#![forbid(unsafe_code)]
#![allow(clippy::unwrap_used, clippy::expect_used)]
use serde_json::json;
use std::collections::BTreeMap;
use std::path::PathBuf;
use tkm_core::model::Host;
use tkm_store::{CalibrationRecord, InstallRecord, NewDecision, Store};

fn decision() -> NewDecision {
    NewDecision {
        model: "model-a".into(),
        purpose: "stuck".into(),
        questions: json!([{"id":"q"}]),
        answers: json!({"q":0.9}),
        cost_usd: 0.001,
        latency_ms: 123,
        shadow: true,
    }
}
fn install() -> InstallRecord {
    InstallRecord {
        host: Host::Claude,
        agent_home: PathBuf::from("/home/agent"),
        files_written: vec![PathBuf::from("/home/agent/settings.json")],
        entry_ids: vec!["tkm-hook".into()],
        backup_paths: vec![PathBuf::from("/home/agent/settings.json.tkm-bak")],
        shim_hashes: BTreeMap::from([("shim".into(), "hash".into())]),
    }
}
#[test]
fn roundtrip_decisions() {
    let store = Store::open_in_memory().unwrap();
    let item = decision();
    let id = store.insert_decision(&item).unwrap();
    let first = store.get_decision(id).unwrap().unwrap();
    assert_eq!(first.model, item.model);
    assert_eq!(first.purpose, item.purpose);
    assert_eq!(first.questions, item.questions);
    assert_eq!(first.answers, item.answers);
    assert_eq!(first.cost_usd.to_bits(), item.cost_usd.to_bits());
    assert_eq!(first.latency_ms, item.latency_ms);
    assert_eq!(first.shadow, item.shadow);
    assert_eq!(first.outcome_label, None);
    let mut other = decision();
    other.model = "model-b".into();
    let other_id = store.insert_decision(&other).unwrap();
    let latest_id = store.insert_decision(&item).unwrap();
    assert_eq!(
        store
            .list_recent_decisions(2)
            .unwrap()
            .iter()
            .map(|r| r.id)
            .collect::<Vec<_>>(),
        vec![latest_id, other_id]
    );
    assert_eq!(
        store
            .list_decisions_by_model_purpose("model-a", "stuck", 10)
            .unwrap()
            .iter()
            .map(|r| r.id)
            .collect::<Vec<_>>(),
        vec![latest_id, id]
    );
    assert!(
        store
            .list_decisions_by_model_purpose("model-a", "other", 10)
            .unwrap()
            .is_empty()
    );
    assert_eq!(store.get_decision(-1).unwrap(), None);
}
#[test]
fn roundtrip_calibration() {
    let store = Store::open_in_memory().unwrap();
    let mut item = CalibrationRecord {
        model_version: "v1".into(),
        purpose: "stuck".into(),
        thresholds: json!({"t_stuck":0.85}),
        sample_count: 200,
        precision: 0.95,
        recall: 0.8,
    };
    store.upsert_calibration(&item).unwrap();
    assert_eq!(
        store.get_calibration("v1", "stuck").unwrap(),
        Some(item.clone())
    );
    item.sample_count = 300;
    item.thresholds = json!({"t_stuck":0.9});
    store.upsert_calibration(&item).unwrap();
    assert_eq!(store.list_calibration().unwrap(), vec![item]);
    assert_eq!(store.get_calibration("v2", "stuck").unwrap(), None);
}
#[test]
fn roundtrip_installs() {
    let store = Store::open_in_memory().unwrap();
    let mut item = install();
    store.upsert_install(&item).unwrap();
    assert_eq!(
        store.get_install(item.host, &item.agent_home).unwrap(),
        Some(item.clone())
    );
    item.entry_ids.push("second".into());
    store.upsert_install(&item).unwrap();
    assert_eq!(store.list_installs().unwrap(), vec![item.clone()]);
    assert_eq!(
        store.get_install(Host::Codex, &item.agent_home).unwrap(),
        None
    );
}
#[test]
fn decision_outcome_label_updates() {
    let store = Store::open_in_memory().unwrap();
    let id = store.insert_decision(&decision()).unwrap();
    store.set_decision_outcome(id, "stuck").unwrap();
    assert_eq!(
        store
            .get_decision(id)
            .unwrap()
            .unwrap()
            .outcome_label
            .as_deref(),
        Some("stuck")
    );
    store.set_decision_outcome(id, "not_stuck").unwrap();
    assert_eq!(
        store
            .get_decision(id)
            .unwrap()
            .unwrap()
            .outcome_label
            .as_deref(),
        Some("not_stuck")
    );
    assert!(store.set_decision_outcome(-1, "missing").is_err());
}
#[test]
fn delete_install_removes_row() {
    let store = Store::open_in_memory().unwrap();
    let item = install();
    store.upsert_install(&item).unwrap();
    assert!(store.delete_install(item.host, &item.agent_home).unwrap());
    assert!(!store.delete_install(item.host, &item.agent_home).unwrap());
    assert_eq!(
        store.get_install(item.host, &item.agent_home).unwrap(),
        None
    );
}
