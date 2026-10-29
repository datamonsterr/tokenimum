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
