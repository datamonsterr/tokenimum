#![forbid(unsafe_code)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::pedantic)]
mod support_ops;
#[test]
fn calibrate_too_few_labels() {
    let s = support_ops::Sandbox::new();
    s.seed_db();
    let out = s.run(&["calibrate", "--purpose", "loop"]);
    assert!(out.status.success());
    assert!(String::from_utf8_lossy(&out.stdout).contains("too few"));
}
#[test]
fn calibrate_loop_fits_labelled_decisions() {
    let s = support_ops::Sandbox::new();
    let paths = tkm_config::paths::Paths::from_dirs(
        s.home.path(),
        Some(&s.home.path().join("data")),
        Some(&s.home.path().join("config")),
        Some(&s.home.path().join("run")),
    );
    let store = tkm_store::Store::open(paths.db).unwrap();
    for i in 0..20 {
        let decision = tkm_store::NewDecision {
            model: "test/model-20261029".into(),
            purpose: "loop_guard".into(),
            questions: serde_json::Value::Null,
            answers: serde_json::json!({"stuck": {"type":"noul", "noul": if i < 10 { 0.95 } else { 0.1 }}}),
            cost_usd: Some(0.0),
            latency_ms: 0,
            shadow: true,
        };
        let id = store.insert_decision(&decision).unwrap();
        store
            .set_decision_outcome(id, if i < 10 { "stuck" } else { "progress" })
            .unwrap();
    }
    let out = s.run(&["calibrate", "--purpose", "loop"]);
    assert!(out.status.success(), "{out:?}");
    let calibration = store
        .get_calibration("test/model-20261029", "loop_guard")
        .unwrap()
        .unwrap();
    assert_eq!(calibration.sample_count, 20);
    assert_eq!(calibration.thresholds["t_stuck"], 0.95);
}
#[test]
fn calibrate_other_purposes_report_missing_labels() {
    let s = support_ops::Sandbox::new();
    s.seed_db();
    for purpose in ["facts", "relevance"] {
        let out = s.run(&["calibrate", "--purpose", purpose]);
        assert!(out.status.success());
        assert!(String::from_utf8_lossy(&out.stdout).contains("too few"));
    }
}

#[test]
fn calibrate_refuses_threshold_at_or_below_t_low_without_overwriting() {
    for threshold in [0.2, 0.3] {
        let s = support_ops::Sandbox::new();
        let paths = tkm_config::paths::Paths::from_dirs(
            s.home.path(),
            Some(&s.home.path().join("data")),
            Some(&s.home.path().join("config")),
            Some(&s.home.path().join("run")),
        );
        let store = tkm_store::Store::open(paths.db).unwrap();
        let prior = tkm_store::CalibrationRecord {
            model_version: "test/model-20261029".into(),
            purpose: "loop_guard".into(),
            thresholds: serde_json::json!({"t_stuck": 0.9, "t_low": 0.3}),
            sample_count: 25,
            precision: 1.0,
            recall: 1.0,
        };
        store.upsert_calibration(&prior).unwrap();
        for i in 0..20 {
            let id = store.insert_decision(&tkm_store::NewDecision {
                model: prior.model_version.clone(), purpose: "loop_guard".into(), questions: serde_json::json!({}),
                answers: serde_json::json!({"stuck": {"type": "noul", "noul": if i < 10 { threshold } else { 0.01 }}}),
                cost_usd: Some(0.0), latency_ms: 0, shadow: true,
            }).unwrap();
            store
                .set_decision_outcome(id, if i < 10 { "stuck" } else { "progress" })
                .unwrap();
        }
        let out = s.run(&["calibrate", "--purpose", "loop"]);
        assert!(out.status.success(), "{out:?}");
        assert!(
            String::from_utf8_lossy(&out.stdout).contains("t_stuck must be greater than t_low"),
            "{out:?}"
        );
        assert_eq!(
            store
                .get_calibration(&prior.model_version, "loop_guard")
                .unwrap(),
            Some(prior)
        );
    }
}

#[test]
fn fixture_outcomes_fit_precision_and_authorize_only_at_target() {
    let s = support_ops::Sandbox::new();
    let fixtures = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/bench");
    s.seed_db();
    let out = s.run(&["calibrate", "--purpose", "loop", "--fixtures", fixtures]);
    assert!(out.status.success(), "{out:?}");
    let paths = tkm_config::paths::Paths::from_dirs(
        s.home.path(),
        Some(&s.home.path().join("data")),
        Some(&s.home.path().join("config")),
        Some(&s.home.path().join("run")),
    );
    let store = tkm_store::Store::open(paths.db).unwrap();
    let records = store.list_calibration().unwrap();
    assert!(!records.is_empty());
    let gate = tkmd::decider::StoreCalibration::default();
    for row in &records {
        gate.refresh(&records);
        assert!(row.sample_count >= 30 && row.precision >= 0.9);
        let key = tkm_decide::calibrate::CalibrationKey {
            model_version: row.model_version.clone(),
            purpose: row.purpose.clone(),
        };
        assert!(tkm_decide::calibrate::CalibrationStore::get(&gate, &key).is_some());
        let mut below = row.clone();
        below.precision = 0.899;
        gate.refresh(&[below]);
        assert!(tkm_decide::calibrate::CalibrationStore::get(&gate, &key).is_none());
    }
}

#[test]
fn calibrate_facts_and_relevance_are_isolated_by_model_and_purpose() {
    let s = support_ops::Sandbox::new();
    let paths = tkm_config::paths::Paths::from_dirs(
        s.home.path(),
        Some(&s.home.path().join("data")),
        Some(&s.home.path().join("config")),
        Some(&s.home.path().join("run")),
    );
    let store = tkm_store::Store::open(paths.db).unwrap();
    for purpose in ["fact_filter", "fact_contradiction", "relevance"] {
        for i in 0..24 {
            let positive = i < 12;
            let value = if positive { 0.95 } else { 0.1 };
            let (answers, labels) = match purpose {
                "fact_filter" => (
                    serde_json::json!({"stable_a":{"type":"noul","noul":value},"safe_a":{"type":"noul","noul":value}}),
                    serde_json::json!({"stable_a":positive,"safe_a":positive}),
                ),
                "fact_contradiction" => (
                    serde_json::json!({"contradicts_a":{"type":"noul","noul":value}}),
                    serde_json::json!({"contradicts_a":positive}),
                ),
                _ => (
                    serde_json::json!({"rel_a":{"type":"score","score":value * 3.0}}),
                    serde_json::json!({"rel_a":positive}),
                ),
            };
            let id = store
                .insert_decision(&tkm_store::NewDecision {
                    model: "good-20261029".into(),
                    purpose: purpose.into(),
                    questions: serde_json::json!({}),
                    answers,
                    cost_usd: Some(0.0),
                    latency_ms: 0,
                    shadow: true,
                })
                .unwrap();
            store.set_decision_outcome(id, &labels.to_string()).unwrap();
        }
    }
    assert!(s.run(&["calibrate", "--purpose", "facts"]).status.success());
    assert!(
        store
            .get_calibration("good-20261029", "fact_filter")
            .unwrap()
            .is_some()
    );
    assert!(
        store
            .get_calibration("good-20261029", "fact_contradiction")
            .unwrap()
            .is_some()
    );
    assert!(
        store
            .get_calibration("good-20261029", "relevance")
            .unwrap()
            .is_none()
    );
    assert!(
        store
            .get_calibration("other-20261029", "fact_filter")
            .unwrap()
            .is_none()
    );
    assert!(
        s.run(&["calibrate", "--purpose", "relevance"])
            .status
            .success()
    );
    assert!(
        store
            .get_calibration("good-20261029", "relevance")
            .unwrap()
            .is_some()
    );
}

#[test]
fn fixture_precision_targets_gate_shadow_for_every_purpose_and_model() {
    let s = support_ops::Sandbox::new();
    s.seed_db();
    let fixtures = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/bench");
    assert!(
        s.run(&["calibrate", "--fixtures", fixtures])
            .status
            .success()
    );
    let paths = tkm_config::paths::Paths::from_dirs(
        s.home.path(),
        Some(&s.home.path().join("data")),
        Some(&s.home.path().join("config")),
        Some(&s.home.path().join("run")),
    );
    let store = tkm_store::Store::open(paths.db).unwrap();
    let records = store.list_calibration().unwrap();
    let gate = tkmd::decider::StoreCalibration::default();
    for purpose in [
        "loop_guard",
        "fact_filter",
        "fact_contradiction",
        "relevance",
    ] {
        let rows: Vec<_> = records
            .iter()
            .filter(|row| row.purpose == purpose)
            .collect();
        assert!(!rows.is_empty(), "missing {purpose}");
        for row in rows {
            assert!(row.precision >= 0.9 && row.recall >= 0.5 && row.sample_count >= 20);
            let key = tkm_decide::calibrate::CalibrationKey {
                model_version: row.model_version.clone(),
                purpose: purpose.into(),
            };
            gate.refresh(std::slice::from_ref(row));
            assert!(tkm_decide::calibrate::CalibrationStore::get(&gate, &key).is_some());
            let mut below = row.clone();
            below.precision = 0.899;
            gate.refresh(&[below]);
            assert!(tkm_decide::calibrate::CalibrationStore::get(&gate, &key).is_none());
        }
        assert!(
            store
                .get_calibration("unreliable-20261029", purpose)
                .unwrap()
                .is_none()
        );
    }
}
