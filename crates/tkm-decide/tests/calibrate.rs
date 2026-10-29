#![forbid(unsafe_code)]
#![allow(clippy::unwrap_used, clippy::expect_used)]
use tkm_decide::calibrate::{
    Action, CalibrationKey, CalibrationStore, FitConfig, InMemoryCalibration, ShadowWindow,
    Thresholds, action_for, agree, fit_lower, fit_upper,
};
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

#[test]
fn calibration_fit_reaches_precision_target_on_noisy_synthetic_labels() {
    let mut state = 42_u64;
    let samples: Vec<_> = (0..2_000)
        .map(|i| {
            state = state
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1);
            let draw = f64::from(u32::try_from(state >> 32).unwrap()) / f64::from(u32::MAX);
            let positive = i % 2 == 0;
            let probability = if positive {
                0.5 + 0.5 * draw
            } else {
                0.65 * draw
            };
            (probability, positive)
        })
        .collect();
    assert!(samples.iter().any(|(p, positive)| *positive && *p < 0.65));
    assert!(samples.iter().any(|(p, positive)| !*positive && *p > 0.5));
    let cfg = FitConfig::default();
    let fit = fit_upper(&samples, &cfg).unwrap();
    assert!(fit.precision >= cfg.precision_target);
    assert!(fit.recall >= cfg.min_recall);
    let impossible = FitConfig {
        min_recall: 1.01,
        ..cfg
    };
    assert!(fit_upper(&samples, &impossible).is_none());
}
