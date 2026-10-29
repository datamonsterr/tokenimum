#![allow(clippy::unwrap_used, clippy::expect_used)]
use tkm_decide::calibrate::*;
#[test]
fn fit_and_calibration_gate() {
    let samples: Vec<_> = (0..100).map(|i| (f64::from(i) / 100.0, i >= 60)).collect();
    let cfg = FitConfig::default();
    let fit = fit_upper(&samples, &cfg).unwrap();
    assert!(fit.meets_target(&cfg) && fit.recall >= cfg.min_recall);
    let lower: Vec<_> = samples.iter().map(|(p, b)| (1.0 - p, *b)).collect();
    assert!(fit_lower(&lower, &cfg).unwrap().meets_target(&cfg));
    let store = InMemoryCalibration::default();
    let key = CalibrationKey {
        model_version: "model-20261029".into(),
        purpose: "stuck".into(),
    };
    assert_eq!(action_for(&store, &key), Action::LogOnly);
    store.put(key.clone(), Thresholds::default());
    assert_eq!(action_for(&store, &key), Action::Act(Thresholds::default()));
}
#[test]
fn shadow_and_agreement_boundaries() {
    let window = ShadowWindow {
        started_ms: 100,
        judged_turns: 200,
    };
    assert!(window.is_shadow(100 + 14 * 86_400_000 - 1));
    assert!(!window.is_shadow(100 + 14 * 86_400_000));
    assert!(
        ShadowWindow {
            judged_turns: 199,
            ..window
        }
        .is_shadow(i64::MAX)
    );
    assert!(agree(Some(0.9), Some(0.8), 0.8));
    assert!(!agree(Some(0.9), None, 0.8));
}
