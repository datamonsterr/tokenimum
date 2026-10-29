#![forbid(unsafe_code)]
#![allow(clippy::unwrap_used, clippy::expect_used)]
use std::{
    collections::{BTreeMap, VecDeque},
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, AtomicUsize, Ordering},
    },
    time::Duration,
};
use tkm_decide::{
    api::DecisionsRequest,
    budget::{DecisionRow, InMemoryLedger, SpendLedger},
    cache::Purpose,
    chain::Clock,
    client::{
        CodeOnlyReason, DecideRequest, DecisionClient, HttpReply, NoDecisionReason, Outcome,
        Transport, TransportError,
    },
    config::{ChainModel, DecideConfig},
    key::{ApiKey, load_key},
};
const GOOD: &str = include_str!("fixtures/api-response-doc.json");
#[derive(Clone)]
enum Script {
    Reply(u16, &'static str),
    Sleep,
    Error,
}
#[derive(Default)]
struct Fake {
    scripts: Mutex<BTreeMap<String, VecDeque<Script>>>,
    bodies: Mutex<Vec<serde_json::Value>>,
    active: AtomicUsize,
    max_active: AtomicUsize,
}
impl Fake {
    fn with_first(script: Script) -> Arc<Self> {
        let fake = Arc::new(Self::default());
        fake.scripts
            .lock()
            .unwrap()
            .insert("first-20261029".into(), VecDeque::from([script]));
        fake
    }
    fn calls(&self) -> usize {
        self.bodies.lock().unwrap().len()
    }
    fn models(&self) -> Vec<String> {
        self.bodies
            .lock()
            .unwrap()
            .iter()
            .map(|v| v["model"].as_str().unwrap().into())
            .collect()
    }
}
#[derive(Clone)]
struct FakeTransport(Arc<Fake>);
impl Transport for FakeTransport {
    async fn post_decision(
        &self,
        _key: &ApiKey,
        body: String,
    ) -> Result<HttpReply, TransportError> {
        let body: serde_json::Value = serde_json::from_str(&body).unwrap();
        let model = body["model"].as_str().unwrap().to_owned();
        self.0.bodies.lock().unwrap().push(body);
        let script = self
            .0
            .scripts
            .lock()
            .unwrap()
            .get_mut(&model)
            .and_then(VecDeque::pop_front)
            .unwrap_or(Script::Reply(200, GOOD));
        self.0.max_active.fetch_max(
            self.0.active.fetch_add(1, Ordering::Relaxed) + 1,
            Ordering::Relaxed,
        );
        let reply = match script {
            Script::Reply(status, body) => {
                tokio::time::sleep(Duration::from_millis(10)).await;
                Ok(HttpReply {
                    status,
                    body: body.into(),
                })
            }
            Script::Sleep => {
                tokio::time::sleep(Duration::from_secs(5)).await;
                Ok(HttpReply {
                    status: 200,
                    body: GOOD.into(),
                })
            }
            Script::Error => Err(TransportError::Request),
        };
        self.0.active.fetch_sub(1, Ordering::Relaxed);
        reply
    }
}
#[derive(Default)]
struct ManualClock(AtomicU64);
impl Clock for ManualClock {
    fn now_ms(&self) -> u64 {
        self.0.load(Ordering::Relaxed)
    }
}
type Client = DecisionClient<FakeTransport, Arc<ManualClock>, Arc<InMemoryLedger>>;
fn config() -> DecideConfig {
    DecideConfig {
        chain: ["first-20261029", "second-20261029"]
            .into_iter()
            .map(|slug| ChainModel {
                slug: slug.into(),
                free_probe: false,
            })
            .collect(),
        ..DecideConfig::default()
    }
}
fn request(purpose: Purpose) -> DecideRequest {
    let wire: DecisionsRequest =
        serde_json::from_str(include_str!("fixtures/api-request-doc.json")).unwrap();
    DecideRequest {
        purpose,
        session_id: "test-session".into(),
        state: "already-redacted synthetic state".into(),
        questions: wire.questions,
    }
}
fn key() -> ApiKey {
    load_key(
        Some(include_str!("fixtures/key-test.txt").into()),
        std::path::Path::new("unused"),
        None,
    )
    .unwrap()
}
fn client(fake: Arc<Fake>) -> Client {
    DecisionClient::new(
        config(),
        Some(key()),
        FakeTransport(fake),
        Arc::new(ManualClock::default()),
        Arc::new(InMemoryLedger::default()),
    )
}
#[tokio::test(start_paused = true)]
async fn http_404_429_500_transport_and_unparsable_body_fall_through() {
    for script in [
        Script::Reply(404, "{}"),
        Script::Reply(429, "{}"),
        Script::Reply(500, "{}"),
        Script::Error,
        Script::Reply(200, "not json"),
    ] {
        let fake = Fake::with_first(script);
        let outcome = client(fake.clone())
            .decide(request(Purpose::LoopGuard))
            .await;
        assert!(
            matches!(outcome, Outcome::Decision(ref d) if d.requested_model == "second-20261029")
        );
        assert_eq!(fake.models(), ["first-20261029", "second-20261029"]);
    }
}
#[tokio::test(start_paused = true)]
async fn timeout_falls_through_without_error() {
    let fake = Fake::with_first(Script::Sleep);
    assert!(
        matches!(client(fake.clone()).decide(request(Purpose::LoopGuard)).await, Outcome::Decision(ref d) if d.requested_model == "second-20261029")
    );
    assert_eq!(fake.calls(), 2);
}
#[tokio::test(start_paused = true)]
async fn invalid_missing_key_probability_choice_score_and_unknown_type_fall_through() {
    for body in [
        include_str!("fixtures/api-invalid-missing-key.json"),
        include_str!("fixtures/api-invalid-probability.json"),
        include_str!("fixtures/api-invalid-choice.json"),
        include_str!("fixtures/api-invalid-score.json"),
        include_str!("fixtures/api-invalid-unknown-type.json"),
    ] {
        let fake = Fake::with_first(Script::Reply(200, body));
        assert!(
            matches!(client(fake.clone()).decide(request(Purpose::LoopGuard)).await, Outcome::Decision(ref d) if d.requested_model == "second-20261029")
        );
        assert_eq!(fake.calls(), 2);
    }
}
#[tokio::test(start_paused = true)]
async fn all_models_failing_returns_no_decision_and_records_each_attempt() {
    let fake = Fake::with_first(Script::Reply(500, "{}"));
    fake.scripts.lock().unwrap().insert(
        "second-20261029".into(),
        VecDeque::from([Script::Reply(404, "{}")]),
    );
    let ledger = Arc::new(InMemoryLedger::default());
    let client = DecisionClient::new(
        config(),
        Some(key()),
        FakeTransport(fake.clone()),
        ManualClock::default(),
        ledger.clone(),
    );
    assert_eq!(
        client.decide(request(Purpose::LoopGuard)).await,
        Outcome::NoDecision(NoDecisionReason::AllFailed)
    );
    assert_eq!(fake.calls(), 2);
    assert_eq!(ledger.rows().len(), 2);
    assert!(
        ledger
            .rows()
            .iter()
            .all(|row| row.outcome != "ok" && !row.cached)
    );
}
#[tokio::test(start_paused = true)]
async fn nonretryable_request_errors_stop_the_chain() {
    for status in [400, 401, 402, 403, 413] {
        let fake = Fake::with_first(Script::Reply(status, "{}"));
        assert_eq!(
            client(fake.clone())
                .decide(request(Purpose::LoopGuard))
                .await,
            Outcome::NoDecision(NoDecisionReason::RequestRejected)
        );
        assert_eq!(fake.calls(), 1);
    }
}
#[tokio::test(start_paused = true)]
async fn daily_and_session_budget_exhaustion_make_zero_transport_calls() {
    for (cost, expected) in [
        (0.50, CodeOnlyReason::OverDaily),
        (0.02, CodeOnlyReason::OverSession),
    ] {
        let fake = Arc::new(Fake::default());
        let ledger = Arc::new(InMemoryLedger::default());
        ledger.record(DecisionRow {
            ts_ms: 0,
            session_id: "test-session".into(),
            purpose: Purpose::LoopGuard,
            requested_model: "first-20261029".into(),
            model_version: None,
            provider: None,
            input_tokens: 0,
            output_tokens: 0,
            cost_usd: Some(cost),
            latency_ms: 0,
            question_names: Vec::new(),
            outcome: "ok".into(),
            cached: false,
        });
        let client = DecisionClient::new(
            config(),
            Some(key()),
            FakeTransport(fake.clone()),
            ManualClock::default(),
            ledger,
        );
        assert_eq!(
            client.decide(request(Purpose::LoopGuard)).await,
            Outcome::CodeOnly(expected)
        );
        assert_eq!(fake.calls(), 0);
    }
}
#[tokio::test(start_paused = true)]
async fn disabled_no_key_and_oversized_state_make_zero_transport_calls() {
    let fake = Arc::new(Fake::default());
    let disabled = DecisionClient::new(
        DecideConfig {
            enabled: false,
            ..config()
        },
        None,
        FakeTransport(fake.clone()),
        ManualClock::default(),
        InMemoryLedger::default(),
    );
    assert_eq!(
        disabled.decide(request(Purpose::LoopGuard)).await,
        Outcome::NoDecision(NoDecisionReason::Disabled)
    );
    let no_key = DecisionClient::new(
        config(),
        None,
        FakeTransport(fake.clone()),
        ManualClock::default(),
        InMemoryLedger::default(),
    );
    assert_eq!(
        no_key.decide(request(Purpose::LoopGuard)).await,
        Outcome::CodeOnly(CodeOnlyReason::NoKey)
    );
    let mut req = request(Purpose::LoopGuard);
    req.state = "界".repeat(6_000);
    assert_eq!(
        client(fake.clone()).decide(req).await,
        Outcome::NoDecision(NoDecisionReason::StateTooLarge)
    );
    assert_eq!(fake.calls(), 0);
}
#[tokio::test(start_paused = true)]
async fn cache_hit_avoids_transport_and_expiry_requests_again_for_each_ttl() {
    for (purpose, ttl) in [
        (Purpose::Relevance, 600_000),
        (Purpose::FactFilter, 86_400_000),
    ] {
        let fake = Arc::new(Fake::default());
        let clock = Arc::new(ManualClock::default());
        let ledger = Arc::new(InMemoryLedger::default());
        let client = DecisionClient::new(
            config(),
            Some(key()),
            FakeTransport(fake.clone()),
            clock.clone(),
            ledger.clone(),
        );
        assert!(
            matches!(client.decide(request(purpose)).await, Outcome::Decision(ref d) if !d.cached)
        );
        clock.0.store(ttl - 1, Ordering::Relaxed);
        assert!(
            matches!(client.decide(request(purpose)).await, Outcome::Decision(ref d) if d.cached)
        );
        assert_eq!(fake.calls(), 1);
        assert_eq!(ledger.rows().len(), 2);
        assert!(ledger.rows()[1].cached);
        assert!(
            ledger.rows()[1]
                .cost_usd
                .is_some_and(|cost| cost.abs() < f64::EPSILON)
        );
        clock.0.store(ttl, Ordering::Relaxed);
        assert!(
            matches!(client.decide(request(purpose)).await, Outcome::Decision(ref d) if !d.cached)
        );
        assert_eq!(fake.calls(), 2);
    }
}
#[tokio::test(start_paused = true)]
async fn request_batches_all_questions_with_privacy_preferences_and_ledger_provenance() {
    let fake = Arc::new(Fake::default());
    let ledger = Arc::new(InMemoryLedger::default());
    let client = DecisionClient::new(
        config(),
        Some(key()),
        FakeTransport(fake.clone()),
        ManualClock::default(),
        ledger.clone(),
    );
    let req = request(Purpose::LoopGuard);
    assert!(matches!(
        client.decide(req.clone()).await,
        Outcome::Decision(_)
    ));
    let bodies = fake.bodies.lock().unwrap();
    assert_eq!(bodies.len(), 1);
    assert_eq!(
        bodies[0]["provider"],
        serde_json::json!({"data_collection":"deny","allow_fallbacks":true})
    );
    assert_eq!(
        bodies[0]["questions"],
        serde_json::to_value(&req.questions).unwrap()
    );
    assert_eq!(bodies[0]["state"], req.state);
    assert!(
        !bodies[0]
            .to_string()
            .contains(include_str!("fixtures/key-test.txt"))
    );
    let rows = ledger.rows();
    assert_eq!(rows[0].question_names, ["is_bug", "team", "urgency"]);
    assert_eq!(
        rows[0].model_version.as_deref(),
        Some("typesafe/jev-1.13-20260917")
    );
    assert_eq!(rows[0].input_tokens, 476);
    assert_eq!(rows[0].output_tokens, 70);
    assert_eq!(rows[0].outcome, "ok");
}
#[tokio::test(start_paused = true)]
async fn agreement_calls_first_two_active_models_concurrently() {
    let fake = Arc::new(Fake::default());
    let (first, second) = client(fake.clone())
        .decide_agreement(request(Purpose::LoopGuard))
        .await;
    assert!(matches!(first, Outcome::Decision(ref d) if d.requested_model == "first-20261029"));
    assert!(matches!(second, Outcome::Decision(ref d) if d.requested_model == "second-20261029"));
    assert_eq!(fake.max_active.load(Ordering::Relaxed), 2);
    assert_eq!(fake.calls(), 2);
}
#[tokio::test(start_paused = true)]
async fn free_model_reprobe_activates_only_a_successful_free_model() {
    let fake = Arc::new(Fake::default());
    let mut config = config();
    config.chain.push(ChainModel {
        slug: "probe:free".into(),
        free_probe: true,
    });
    let client = DecisionClient::new(
        config,
        Some(key()),
        FakeTransport(fake.clone()),
        ManualClock::default(),
        InMemoryLedger::default(),
    );
    client.decide(request(Purpose::LoopGuard)).await;
    assert_eq!(fake.models(), ["first-20261029"]);
    let probed = client.reprobe_free(request(Purpose::LoopGuard)).await;
    assert!(matches!(probed.as_slice(), [Outcome::Decision(_)]));
    client.decide(request(Purpose::LoopGuard)).await;
    assert_eq!(
        fake.models(),
        ["first-20261029", "probe:free", "probe:free"]
    );
}

#[tokio::test(start_paused = true)]
async fn fallback_cache_hit_skips_new_primary_request() {
    let fake = Fake::with_first(Script::Reply(404, "{}"));
    let client = client(fake.clone());
    assert!(
        matches!(client.decide(request(Purpose::Relevance)).await, Outcome::Decision(ref d) if d.requested_model == "second-20261029" && !d.cached)
    );
    assert!(
        matches!(client.decide(request(Purpose::Relevance)).await, Outcome::Decision(ref d) if d.requested_model == "second-20261029" && d.cached)
    );
    assert_eq!(fake.calls(), 2);
}

#[tokio::test(start_paused = true)]
async fn client_skips_failed_model_until_cooldown_expires_then_retries() {
    let fake = Arc::new(Fake::default());
    fake.scripts.lock().unwrap().insert(
        "first-20261029".into(),
        VecDeque::from([
            Script::Reply(500, "{}"),
            Script::Reply(500, "{}"),
            Script::Reply(500, "{}"),
        ]),
    );
    let clock = Arc::new(ManualClock::default());
    let client = DecisionClient::new(
        config(),
        Some(key()),
        FakeTransport(fake.clone()),
        clock.clone(),
        InMemoryLedger::default(),
    );
    for now in [0, 100_000, 200_000] {
        clock.0.store(now, Ordering::Relaxed);
        assert!(
            matches!(client.decide(request(Purpose::LoopGuard)).await, Outcome::Decision(ref d) if d.requested_model == "second-20261029")
        );
    }
    assert_eq!(fake.calls(), 6);
    clock.0.store(1_099_999, Ordering::Relaxed);
    assert!(
        matches!(client.decide(request(Purpose::LoopGuard)).await, Outcome::Decision(ref d) if d.requested_model == "second-20261029")
    );
    assert_eq!(fake.calls(), 7);
    clock.0.store(1_100_000, Ordering::Relaxed);
    assert!(
        matches!(client.decide(request(Purpose::LoopGuard)).await, Outcome::Decision(ref d) if d.requested_model == "first-20261029")
    );
    assert_eq!(fake.calls(), 8);
}

#[tokio::test(start_paused = true)]
async fn failed_answer_cost_is_recorded_and_budget_checked_before_retry() {
    let fake = Fake::with_first(Script::Reply(
        200,
        include_str!("fixtures/api-invalid-probability.json"),
    ));
    let ledger = Arc::new(InMemoryLedger::default());
    let client = DecisionClient::new(
        DecideConfig {
            session_cap_usd: 0.000_01,
            ..config()
        },
        Some(key()),
        FakeTransport(fake.clone()),
        ManualClock::default(),
        ledger.clone(),
    );
    assert_eq!(
        client.decide(request(Purpose::LoopGuard)).await,
        Outcome::CodeOnly(CodeOnlyReason::OverSession)
    );
    assert_eq!(fake.calls(), 1);
    assert_eq!(ledger.rows()[0].outcome, "invalid_answer");
    assert!(ledger.spent_session_usd("test-session") > 0.000_01);
}

#[tokio::test(start_paused = true)]
async fn agreement_and_free_probes_obey_no_key_gate() {
    let fake = Arc::new(Fake::default());
    let client = DecisionClient::new(
        config(),
        None,
        FakeTransport(fake.clone()),
        ManualClock::default(),
        InMemoryLedger::default(),
    );
    let expected = Outcome::CodeOnly(CodeOnlyReason::NoKey);
    assert_eq!(
        client.decide_agreement(request(Purpose::LoopGuard)).await,
        (expected.clone(), expected.clone())
    );
    assert_eq!(
        client.reprobe_free(request(Purpose::LoopGuard)).await,
        [expected]
    );
    assert_eq!(fake.calls(), 0);
}

fn probe_config() -> DecideConfig {
    let mut cfg = config();
    for slug in ["probe-one:free", "probe-two:free"] {
        cfg.chain.push(ChainModel {
            slug: slug.into(),
            free_probe: true,
        });
    }
    cfg
}

#[tokio::test(start_paused = true)]
async fn rejected_free_probe_preserves_active_slots_and_stops_probing() {
    for status in [400, 401, 402, 403, 413] {
        let fake = Arc::new(Fake::default());
        let client = DecisionClient::new(
            probe_config(),
            Some(key()),
            FakeTransport(fake.clone()),
            ManualClock::default(),
            InMemoryLedger::default(),
        );
        let initial = client.reprobe_free(request(Purpose::LoopGuard)).await;
        assert!(
            initial
                .iter()
                .all(|outcome| matches!(outcome, Outcome::Decision(_)))
        );
        fake.scripts.lock().unwrap().insert(
            "probe-one:free".into(),
            VecDeque::from([Script::Reply(status, "{}")]),
        );
        assert_eq!(
            client.reprobe_free(request(Purpose::LoopGuard)).await,
            [Outcome::NoDecision(NoDecisionReason::RequestRejected)]
        );
        assert_eq!(fake.calls(), 3);
        let (first, second) = client.decide_agreement(request(Purpose::LoopGuard)).await;
        assert!(matches!(first, Outcome::Decision(ref d) if d.requested_model == "probe-one:free"));
        assert!(
            matches!(second, Outcome::Decision(ref d) if d.requested_model == "probe-two:free")
        );
    }
}

#[derive(Default)]
struct ProbeBudgetLedger(AtomicUsize);
impl SpendLedger for ProbeBudgetLedger {
    fn spent_today_usd(&self, _now_ms: u64) -> f64 {
        0.0
    }
    fn spent_session_usd(&self, _session_id: &str) -> f64 {
        // Two probes use three checks (gate + attempts). Block only the second
        // attempt of the next probe run, then allow requests again.
        if self.0.fetch_add(1, Ordering::Relaxed) == 5 {
            1.0
        } else {
            0.0
        }
    }
    fn record(&self, _row: DecisionRow) {}
}

#[tokio::test(start_paused = true)]
async fn budget_stop_during_free_probes_preserves_the_unattempted_active_slot() {
    let fake = Arc::new(Fake::default());
    let client = DecisionClient::new(
        probe_config(),
        Some(key()),
        FakeTransport(fake.clone()),
        ManualClock::default(),
        ProbeBudgetLedger::default(),
    );
    assert_eq!(
        client.reprobe_free(request(Purpose::LoopGuard)).await.len(),
        2
    );
    let outcomes = client.reprobe_free(request(Purpose::LoopGuard)).await;
    assert!(matches!(
        outcomes.as_slice(),
        [
            Outcome::Decision(_),
            Outcome::CodeOnly(CodeOnlyReason::OverSession)
        ]
    ));
    assert_eq!(fake.calls(), 3);
    let (first, second) = client.decide_agreement(request(Purpose::LoopGuard)).await;
    assert!(matches!(first, Outcome::Decision(ref d) if d.requested_model == "probe-one:free"));
    assert!(matches!(second, Outcome::Decision(ref d) if d.requested_model == "probe-two:free"));
}

#[tokio::test(start_paused = true)]
async fn failed_free_probe_deactivates_the_slot() {
    let fake = Arc::new(Fake::default());
    let client = DecisionClient::new(
        probe_config(),
        Some(key()),
        FakeTransport(fake.clone()),
        ManualClock::default(),
        InMemoryLedger::default(),
    );
    client.reprobe_free(request(Purpose::LoopGuard)).await;
    fake.scripts.lock().unwrap().insert(
        "probe-one:free".into(),
        VecDeque::from([Script::Reply(404, "{}")]),
    );
    let outcomes = client.reprobe_free(request(Purpose::LoopGuard)).await;
    assert!(matches!(
        outcomes.as_slice(),
        [
            Outcome::NoDecision(NoDecisionReason::AllFailed),
            Outcome::Decision(_)
        ]
    ));
    assert!(matches!(client.decide(request(Purpose::LoopGuard)).await,
        Outcome::Decision(ref d) if d.requested_model == "probe-two:free"));
}

#[tokio::test(start_paused = true)]
async fn agreement_prefers_jev_over_the_fallback_and_charges_both_costs() {
    let fake = Arc::new(Fake::default());
    let ledger = Arc::new(InMemoryLedger::default());
    let config = DecideConfig {
        session_cap_usd: 0.000_000_001,
        ..DecideConfig::default()
    };
    let client = DecisionClient::new(
        config,
        Some(key()),
        FakeTransport(fake.clone()),
        ManualClock::default(),
        ledger.clone(),
    );
    let _ = client.decide_agreement(request(Purpose::LoopGuard)).await;
    assert_eq!(
        fake.models(),
        [
            "inception/mercury-decide-20260930",
            "typesafe/jev-1.14-20261014"
        ]
    );
    assert_eq!(ledger.rows().len(), 2);
    let total: f64 = ledger.rows().iter().filter_map(|row| row.cost_usd).sum();
    assert!(total > 0.0);
    assert_eq!(
        client.decide_agreement(request(Purpose::LoopGuard)).await,
        (
            Outcome::CodeOnly(CodeOnlyReason::OverSession),
            Outcome::CodeOnly(CodeOnlyReason::OverSession)
        )
    );
    assert_eq!(fake.calls(), 2);
}
