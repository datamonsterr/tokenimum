//! Live agreement uses the actual client and a concurrent, mocked transport.
#![forbid(unsafe_code)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::pedantic)]
mod common;
use common::*;
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};
use tkm_decide::{
    budget::SpendLedger,
    client::{HttpReply, Transport, TransportError},
    config::{ChainModel, DecideConfig},
    key::{ApiKey, load_key},
};
use tkmd::{
    clock::ManualClock,
    decider::{ClientDecider, StoreLedger},
    settings::Settings,
};
const MERCURY: &str = "inception/mercury-decide-20260930";
const JEV: &str = "typesafe/jev-1.14-20261014";
#[derive(Clone, Copy)]
enum Case {
    Agree,
    RedirectDisagrees,
    VerdictDisagrees,
    Failure,
    Uncalibrated,
}
struct Fake {
    case: Case,
    seen: Mutex<Vec<String>>,
    rendezvous: tokio::sync::Barrier,
}
#[derive(Clone)]
struct Mock(Arc<Fake>);
impl Transport for Mock {
    async fn post_decision(&self, _: &ApiKey, body: String) -> Result<HttpReply, TransportError> {
        let body: serde_json::Value = serde_json::from_str(&body).unwrap();
        let model = body["model"].as_str().unwrap().to_owned();
        self.0.seen.lock().unwrap().push(model.clone());
        // Neither response can complete until both transports have started.
        self.0.rendezvous.wait().await;
        if model == JEV && matches!(self.0.case, Case::Failure) {
            return Err(TransportError::Request);
        }
        let redirect = if model == JEV && matches!(self.0.case, Case::RedirectDisagrees) {
            "check_env"
        } else {
            "reread_error"
        };
        let stuck = if model == JEV && matches!(self.0.case, Case::VerdictDisagrees) {
            0.1
        } else {
            0.95
        };
        Ok(HttpReply { status:200, body:serde_json::json!({"id":"mock","model":model,"provider":"test","answers":{
            "stuck":{"type":"noul","noul":stuck},"progress":{"type":"noul","noul":0.1},"redirect":{"type":"choice","choice":redirect}},
            "usage":{"input_tokens":20,"output_tokens":0,"cost":0.00001}}).to_string() })
    }
}

#[tokio::test]
async fn live_agreement_acts_only_on_consensus_records_disagreement_and_fails_open() {
    for case in [
        Case::Agree,
        Case::RedirectDisagrees,
        Case::VerdictDisagrees,
        Case::Failure,
        Case::Uncalibrated,
    ] {
        let env = Env::new();
        env.seed_envelope(FIXTURE_REPO, tkm_proto::Host::Claude, &[5; 5]);
        let rows:Vec<_> = [MERCURY,JEV].into_iter().filter(|model| !(matches!(case, Case::Uncalibrated) && *model == JEV)).map(|model| tkm_store::CalibrationRecord {
            model_version:model.into(), purpose:"loop_guard".into(), thresholds:serde_json::json!({"t_stuck":0.85,"t_progress":0.3,"t_low":0.3,"t_high":0.8}), sample_count:200, precision:0.95, recall:0.6,
        }).collect();
        for row in &rows {
            env.store().upsert_calibration(row).unwrap();
        }
        let ledger = Arc::new(StoreLedger::default());
        let fake = Arc::new(Fake {
            case,
            seen: Mutex::new(Vec::new()),
            rendezvous: tokio::sync::Barrier::new(2),
        });
        let config = DecideConfig {
            agreement: true,
            chain: [MERCURY, JEV]
                .into_iter()
                .map(|slug| ChainModel {
                    slug: slug.into(),
                    free_probe: false,
                })
                .collect(),
            ..DecideConfig::default()
        };
        let decider = Arc::new(ClientDecider::with_clock(
            config.clone(),
            Some(load_key(Some("fixture-key".into()), std::path::Path::new(""), None).unwrap()),
            Mock(fake.clone()),
            ManualClock::new(1_800_000_000_000),
            ledger.clone(),
        ));
        let daemon = daemon_with(
            &env,
            Settings {
                decide: config,
                shadow_days: 0,
                shadow_turns: 0,
                ..Settings::default()
            },
            decider,
        );
        daemon.start_workers();
        daemon.handle(session_start()).await;
        daemon.handle(prompt()).await;
        for _ in 0..18 {
            daemon.handle_tool_done(&failing_tool_done());
        }
        tokio::time::timeout(Duration::from_secs(5), daemon.settle())
            .await
            .unwrap();
        let reply = daemon.handle_tool_done(&failing_tool_done());
        assert_eq!(
            additional_context(&reply).is_some(),
            matches!(case, Case::Agree)
        );
        assert_eq!(fake.seen.lock().unwrap().len(), 2);
        let expected_cost = if matches!(case, Case::Failure) {
            0.00001
        } else {
            0.00002
        };
        assert!(
            (ledger.spent_session_usd("claude:e928a2cf-e31b-4154-805b-919991d042ea")
                - expected_cost)
                .abs()
                < 1e-10
        );
        daemon.flush_now().await;
        let db = tkm_store::open_read_only(&env.paths.db).unwrap();
        let count: i64 = db
            .query_row(
                "SELECT COUNT(*) FROM decisions WHERE purpose='loop_guard'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, if matches!(case, Case::Failure) { 1 } else { 2 });
        let audit: String = db
            .query_row(
                "SELECT inputs FROM events WHERE kind='decide.agreement'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        let audit: serde_json::Value = serde_json::from_str(&audit).unwrap();
        assert_eq!(audit["consensus"], matches!(case, Case::Agree));
        if !matches!(case, Case::Agree) {
            let acted: i64 = db
                .query_row("SELECT COUNT(*) FROM decisions WHERE shadow=0", [], |r| {
                    r.get(0)
                })
                .unwrap();
            assert_eq!(acted, 0);
        }
    }
}
