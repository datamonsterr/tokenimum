#![forbid(unsafe_code)]
#![allow(clippy::unwrap_used, clippy::expect_used)]
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
            cost_usd: 0.0,
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
fn calibrate_other_purposes_explain_unavailability() {
    let s = support_ops::Sandbox::new();
    s.seed_db();
    for purpose in ["facts", "relevance"] {
        let out = s.run(&["calibrate", "--purpose", purpose]);
        assert!(out.status.success());
        assert!(String::from_utf8_lossy(&out.stdout).contains("no label contract"));
    }
}
