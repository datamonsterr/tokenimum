//! Calibration authorization must never leak across decision purposes.
#![forbid(unsafe_code)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::pedantic)]
use tkm_decide::calibrate::{CalibrationKey, CalibrationStore, Thresholds};
use tkmd::decider::StoreCalibration;

#[test]
fn all_purposes_require_scored_precision_and_unscored_put_revokes_authorization() {
    for purpose in [
        "loop_guard",
        "fact_filter",
        "fact_contradiction",
        "relevance",
    ] {
        let gate = StoreCalibration::default();
        let key = CalibrationKey {
            model_version: "m-20261029".into(),
            purpose: purpose.into(),
        };
        let mut row = tkm_store::CalibrationRecord {
            model_version: key.model_version.clone(),
            purpose: key.purpose.clone(),
            thresholds: serde_json::json!({"t_stuck":0.85,"t_progress":0.3,"t_low":0.3,"t_high":0.8}),
            sample_count: 200,
            precision: 0.9,
            recall: 0.6,
        };
        gate.refresh(&[row.clone()]);
        assert!(gate.get(&key).is_some());
        row.precision = 0.899;
        gate.refresh(&[row.clone()]);
        assert!(gate.get(&key).is_none());
        row.precision = 0.9;
        gate.refresh(&[row.clone()]);
        gate.put(key.clone(), Thresholds::default());
        assert!(gate.get(&key).is_none());
        row.sample_count = 0;
        gate.refresh(&[row]);
        assert!(gate.get(&key).is_none());
    }
}

#[test]
fn initial_shadow_windows_do_not_borrow_another_purposes_history() {
    let store = tkm_store::Store::open_in_memory().unwrap();
    for i in 0..200 {
        let row = tkm_store::NewDecision {
            model: "m".into(),
            purpose: "loop_guard".into(),
            questions: serde_json::json!({}),
            answers: serde_json::json!({}),
            cost_usd: Some(0.0),
            latency_ms: 0,
            shadow: true,
        };
        store
            .insert_decision_with_context(
                &row,
                Some(tkm_core::model::Timestamp(i)),
                None,
                None,
                Some(&i.to_string()),
            )
            .unwrap();
    }
    let now = 15 * 86_400_000;
    assert!(tkmd::decider::purpose_shadow(
        &store,
        "fact_filter",
        now,
        14,
        200
    ));
    assert!(tkmd::decider::purpose_shadow(
        &store,
        "relevance",
        now,
        14,
        200
    ));
    assert!(!tkmd::decider::purpose_shadow(
        &store,
        "loop_guard",
        now,
        14,
        200
    ));
}
