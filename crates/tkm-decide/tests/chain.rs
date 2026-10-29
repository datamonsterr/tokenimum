#![forbid(unsafe_code)]
#![allow(clippy::unwrap_used, clippy::expect_used)]
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};
use tkm_decide::{
    chain::{Clock, ModelChain, is_non_retryable},
    config::{ChainModel, DecideConfig},
};
#[derive(Default)]
struct ManualClock(AtomicU64);
impl Clock for ManualClock {
    fn now_ms(&self) -> u64 {
        self.0.load(Ordering::Relaxed)
    }
}
fn model(slug: &str, free_probe: bool) -> ChainModel {
    ChainModel {
        slug: slug.into(),
        free_probe,
    }
}
#[test]
fn three_failures_within_five_minutes_skip_for_fifteen_minutes_then_retry() {
    let clock = Arc::new(ManualClock::default());
    let chain = ModelChain::new(&[model("a-20261029", false)], clock.clone());
    chain.record_failure("a-20261029");
    clock.0.store(100_000, Ordering::Relaxed);
    chain.record_failure("a-20261029");
    clock.0.store(200_000, Ordering::Relaxed);
    chain.record_failure("a-20261029");
    assert!(chain.active_models().is_empty());
    clock.0.store(1_099_999, Ordering::Relaxed);
    assert!(chain.active_models().is_empty());
    clock.0.store(1_100_000, Ordering::Relaxed);
    assert_eq!(chain.active_models(), ["a-20261029"]);
}
#[test]
fn failures_outside_five_minutes_do_not_open_skip_window() {
    let clock = Arc::new(ManualClock::default());
    let chain = ModelChain::new(&[model("a-20261029", false)], clock.clone());
    for now in [0, 300_001, 600_002] {
        clock.0.store(now, Ordering::Relaxed);
        chain.record_failure("a-20261029");
    }
    assert_eq!(chain.active_models(), ["a-20261029"]);
}
#[test]
fn free_slot_is_inactive_until_successful_reprobe_and_moves_to_front() {
    let chain = ModelChain::new(
        &[model("paid-20261029", false), model("probe:free", true)],
        ManualClock::default(),
    );
    assert_eq!(chain.active_models(), ["paid-20261029"]);
    chain.reprobe_free("probe:free", false);
    assert_eq!(chain.active_models(), ["paid-20261029"]);
    chain.reprobe_free("probe:free", true);
    assert_eq!(chain.active_models(), ["probe:free", "paid-20261029"]);
    chain.reprobe_free("probe:free", false);
    assert_eq!(chain.active_models(), ["paid-20261029"]);
}
#[test]
fn free_suffix_is_probe_only_even_without_config_flag() {
    let chain = ModelChain::new(&[model("probe:free", false)], ManualClock::default());
    assert!(chain.active_models().is_empty());
}
#[test]
fn request_errors_are_nonretryable_but_rate_limits_and_server_errors_retry() {
    for status in [400, 401, 402, 403, 413] {
        assert!(is_non_retryable(status));
    }
    for status in [404, 429, 500, 502, 503, 524, 529] {
        assert!(!is_non_retryable(status));
    }
}
#[test]
fn defaults_are_pinned_and_match_spec() {
    let config = DecideConfig::default();
    assert!(config.validate().is_ok());
    assert_eq!(
        config
            .chain
            .iter()
            .map(|m| m.slug.as_str())
            .collect::<Vec<_>>(),
        [
            "inception/mercury-decide:free",
            "inception/mercury-decide-20260930",
            "perplexity/pplx-decider-flash-v1.2-20261016",
            "typesafe/jev-1.14-20261014"
        ]
    );
    assert!(config.chain[0].free_probe);
    assert_eq!(config.timeout_ms, 1_500);
    assert!((config.daily_cap_usd - 0.50).abs() < f64::EPSILON);
    assert!((config.session_cap_usd - 0.02).abs() < f64::EPSILON);
    assert_eq!(config.max_state_bytes, 16_384);
    assert!(config.enabled);
    assert!(!config.agreement);
    assert_eq!(serde_json::from_str::<DecideConfig>("{}").unwrap(), config);
}
#[test]
fn aliases_and_undated_slugs_are_rejected_but_free_slots_are_allowed() {
    for slug in [
        "~typesafe/jev-latest",
        "model-latest",
        "model",
        "model-1234567",
        "model-abcdefgh",
        "~model-20261029",
        "~model:free",
    ] {
        let config = DecideConfig {
            chain: vec![model(slug, false)],
            ..DecideConfig::default()
        };
        assert!(config.validate().is_err(), "accepted {slug}");
    }
    let config = DecideConfig {
        chain: vec![model("model:free", true)],
        ..DecideConfig::default()
    };
    assert!(config.validate().is_ok());
}
