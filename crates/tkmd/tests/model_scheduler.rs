#![forbid(unsafe_code)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::pedantic)]
mod common;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;
use tkm_decide::{
    client::HttpTransport,
    config::{ChainModel, DecideConfig},
};
use tkmd::{
    clock::ManualClock,
    daemon::{Daemon, DaemonOptions},
    decider::{ClientDecider, Decider, StoreLedger},
};

fn config() -> DecideConfig {
    DecideConfig {
        chain: vec![
            ChainModel {
                slug: "test/paid-20261029".into(),
                free_probe: false,
            },
            ChainModel {
                slug: "test/probe:free".into(),
                free_probe: true,
            },
        ],
        ..DecideConfig::default()
    }
}
fn client(endpoint: &str, clock: Arc<ManualClock>, ledger: Arc<StoreLedger>) -> Arc<dyn Decider> {
    let key = tkm_decide::key::load_key(
        Some("fixture-key".into()),
        std::path::Path::new("unused"),
        None,
    )
    .unwrap();
    Arc::new(ClientDecider::with_clock(
        config(),
        Some(key),
        HttpTransport::with_endpoint(endpoint).unwrap(),
        clock,
        ledger,
    ))
}
fn daemon(
    env: &common::Env,
    clock: Arc<ManualClock>,
    decider: Arc<dyn Decider>,
    ledger: Arc<StoreLedger>,
) -> Arc<Daemon> {
    let mut opts = DaemonOptions::new(env.paths.clone());
    opts.clock = clock;
    opts.decider = decider;
    opts.ledger = ledger;
    Daemon::new(opts)
}
fn body() -> String {
    serde_json::json!({"model":"test/probe-20261029","answers":{"available":{"type":"noul","noul":1.0}},"usage":{"input_tokens":1,"output_tokens":1,"cost":0.0}}).to_string()
}

#[tokio::test]
async fn daily_probe_promotes_free_model_and_restart_restores_without_http() {
    let env = common::Env::new();
    let clock = Arc::new(ManualClock::new(1_800_000_000_000));
    let (endpoint, hits) = common::fake_openrouter(200, body(), Duration::ZERO).await;
    let ledger = Arc::new(StoreLedger::default());
    let decider = client(&endpoint, clock.clone(), ledger.clone());
    let daemon = daemon(&env, clock.clone(), decider.clone(), ledger.clone());
    assert_eq!(decider.active_models(), ["test/paid-20261029"]);
    daemon.reprobe_models().await;
    assert_eq!(
        decider.active_models(),
        ["test/probe:free", "test/paid-20261029"]
    );
    daemon.reprobe_models().await;
    assert_eq!(hits.load(Ordering::SeqCst), 1);
    let restored = client(&endpoint, clock.clone(), ledger.clone());
    let restarted = self::daemon(&env, clock.clone(), restored.clone(), ledger);
    restarted.reprobe_models().await;
    assert_eq!(restored.active_models()[0], "test/probe:free");
    assert_eq!(hits.load(Ordering::SeqCst), 1);
    clock.advance(Duration::from_millis(86_399_999));
    restarted.reprobe_models().await;
    assert_eq!(hits.load(Ordering::SeqCst), 1);
    clock.advance(Duration::from_millis(1));
    restarted.reprobe_models().await;
    assert_eq!(hits.load(Ordering::SeqCst), 2);
    assert_eq!(
        env.store().free_model_probes().unwrap()[0].model,
        "test/probe:free"
    );
}

#[tokio::test]
async fn failed_http_is_fail_open_and_retries_next_day() {
    let env = common::Env::new();
    let clock = Arc::new(ManualClock::new(1_800_000_000_000));
    let (endpoint, hits) = common::fake_openrouter(503, "unavailable".into(), Duration::ZERO).await;
    let ledger = Arc::new(StoreLedger::default());
    let decider = client(&endpoint, clock.clone(), ledger.clone());
    let daemon = daemon(&env, clock.clone(), decider.clone(), ledger);
    daemon.reprobe_models().await;
    assert_eq!(decider.active_models(), ["test/paid-20261029"]);
    daemon.reprobe_models().await;
    assert_eq!(hits.load(Ordering::SeqCst), 1);
    clock.advance(Duration::from_hours(24));
    daemon.reprobe_models().await;
    assert_eq!(hits.load(Ordering::SeqCst), 2);
    assert!(!env.store().free_model_probes().unwrap()[0].succeeded);
}

#[test]
fn daily_claim_is_atomic_between_connections() {
    let env = common::Env::new();
    let first = env.store();
    let second = env.store();
    assert!(first.claim_free_model_probe(100).unwrap());
    assert!(!second.claim_free_model_probe(100).unwrap());
    assert!(!second.claim_free_model_probe(86_400_099).unwrap());
    assert!(second.claim_free_model_probe(86_400_100).unwrap());
}

#[tokio::test]
async fn workers_schedule_a_probe_without_an_explicit_doctor_call() {
    let env = common::Env::new();
    let clock = Arc::new(ManualClock::new(1_800_000_000_000));
    let (endpoint, hits) = common::fake_openrouter(200, body(), Duration::ZERO).await;
    let ledger = Arc::new(StoreLedger::default());
    let decider = client(&endpoint, clock.clone(), ledger.clone());
    let daemon = daemon(&env, clock, decider, ledger);
    daemon.start_workers();
    assert!(common::eventually(3, || hits.load(Ordering::SeqCst) == 1).await);
    assert!(common::eventually(3, || env.store().free_model_probes().unwrap().len() == 1).await);
    daemon.shutdown().await;
}

#[tokio::test]
async fn locked_store_skips_http_and_keeps_paid_fallback_available() {
    let env = common::Env::new();
    let clock = Arc::new(ManualClock::new(1_800_000_000_000));
    let (endpoint, hits) = common::fake_openrouter(200, body(), Duration::ZERO).await;
    let ledger = Arc::new(StoreLedger::default());
    let decider = client(&endpoint, clock.clone(), ledger.clone());
    let mut options = DaemonOptions::new(env.paths.clone());
    drop(env.store());
    let writer = rusqlite::Connection::open(&env.paths.db).unwrap();
    writer.execute_batch("BEGIN IMMEDIATE").unwrap();
    options.backing = Arc::new(tkmd::backing::SqliteBacking::with_busy_timeout(
        env.paths.db.clone(),
        Duration::from_millis(1),
    ));
    options.clock = clock;
    options.decider = decider.clone();
    options.ledger = ledger;
    let daemon = Daemon::new(options);
    daemon.reprobe_models().await;
    assert_eq!(hits.load(Ordering::SeqCst), 0);
    assert_eq!(decider.active_models(), ["test/paid-20261029"]);
    writer.execute_batch("ROLLBACK").unwrap();
}
