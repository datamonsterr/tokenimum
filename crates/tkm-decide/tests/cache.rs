#![forbid(unsafe_code)]
#![allow(clippy::unwrap_used, clippy::expect_used)]
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};
use tkm_decide::{
    api::{DecisionsRequest, DecisionsResponse},
    cache::{DecisionCache, Purpose},
    chain::Clock,
};
#[derive(Default)]
struct ManualClock(AtomicU64);
impl Clock for ManualClock {
    fn now_ms(&self) -> u64 {
        self.0.load(Ordering::Relaxed)
    }
}
#[test]
fn relevance_expires_at_ten_minutes_and_fact_filter_at_twenty_four_hours() {
    let req: DecisionsRequest =
        serde_json::from_str(include_str!("fixtures/api-request-doc.json")).unwrap();
    let response: DecisionsResponse =
        serde_json::from_str(include_str!("fixtures/api-response-doc.json")).unwrap();
    for (purpose, ttl) in [
        (Purpose::Relevance, 600_000),
        (Purpose::FactFilter, 86_400_000),
    ] {
        let clock = Arc::new(ManualClock::default());
        let cache = DecisionCache::new(clock.clone());
        cache.insert("a", "state", &req.questions, purpose, response.clone());
        clock.0.store(ttl - 1, Ordering::Relaxed);
        assert!(cache.get("a", "state", &req.questions, purpose).is_some());
        clock.0.store(ttl, Ordering::Relaxed);
        assert!(cache.get("a", "state", &req.questions, purpose).is_none());
    }
}
#[test]
fn cache_key_includes_model_state_and_all_questions() {
    let mut req: DecisionsRequest =
        serde_json::from_str(include_str!("fixtures/api-request-doc.json")).unwrap();
    let response = serde_json::from_str(include_str!("fixtures/api-response-doc.json")).unwrap();
    let cache = DecisionCache::new(ManualClock::default());
    cache.insert("a", "state", &req.questions, Purpose::Relevance, response);
    assert!(
        cache
            .get("a", "state", &req.questions, Purpose::Relevance)
            .is_some()
    );
    assert!(
        cache
            .get("b", "state", &req.questions, Purpose::Relevance)
            .is_none()
    );
    assert!(
        cache
            .get("a", "other state", &req.questions, Purpose::Relevance)
            .is_none()
    );
    req.questions.remove("team");
    assert!(
        cache
            .get("a", "state", &req.questions, Purpose::Relevance)
            .is_none()
    );
}
#[test]
fn purpose_names_are_stable_and_loop_judgments_are_not_cached() {
    assert_eq!(Purpose::LoopGuard.as_str(), "loop_guard");
    assert_eq!(Purpose::Relevance.as_str(), "relevance");
    assert_eq!(Purpose::FactFilter.as_str(), "fact_filter");
    assert_eq!(Purpose::Redirect.as_str(), "redirect");
    assert_eq!(Purpose::LoopGuard.ttl_ms(), 0);
    assert_eq!(Purpose::Redirect.ttl_ms(), 0);
}

#[test]
fn shorter_purpose_cannot_inherit_fact_filter_lifetime() {
    let req: DecisionsRequest =
        serde_json::from_str(include_str!("fixtures/api-request-doc.json")).unwrap();
    let response = serde_json::from_str(include_str!("fixtures/api-response-doc.json")).unwrap();
    let clock = Arc::new(ManualClock::default());
    let cache = DecisionCache::new(clock.clone());
    cache.insert("a", "state", &req.questions, Purpose::FactFilter, response);
    clock.0.store(600_000, Ordering::Relaxed);
    assert!(
        cache
            .get("a", "state", &req.questions, Purpose::Relevance)
            .is_none()
    );
    assert!(
        cache
            .get("a", "state", &req.questions, Purpose::FactFilter)
            .is_some()
    );
}

#[test]
fn distinct_states_keep_independent_responses_and_equal_inputs_hit() {
    let req: DecisionsRequest =
        serde_json::from_str(include_str!("fixtures/api-request-doc.json")).unwrap();
    let first: DecisionsResponse =
        serde_json::from_str(include_str!("fixtures/api-response-doc.json")).unwrap();
    let mut second = first.clone();
    second.id = Some("second-state-response".into());
    let cache = DecisionCache::new(ManualClock::default());
    cache.insert(
        "model",
        "first state",
        &req.questions,
        Purpose::Relevance,
        first.clone(),
    );
    assert!(
        cache
            .get("model", "second state", &req.questions, Purpose::Relevance)
            .is_none()
    );
    cache.insert(
        "model",
        "second state",
        &req.questions,
        Purpose::Relevance,
        second.clone(),
    );
    let equal_questions = req.questions.clone();
    assert_eq!(
        cache.get("model", "first state", &equal_questions, Purpose::Relevance),
        Some(first)
    );
    assert_eq!(
        cache.get(
            "model",
            "second state",
            &equal_questions,
            Purpose::Relevance
        ),
        Some(second)
    );
}
