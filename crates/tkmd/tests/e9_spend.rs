#![forbid(unsafe_code)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::pedantic)]
use std::{collections::BTreeMap, fs, os::unix::fs::PermissionsExt, sync::Arc};
use tkm_decide::{
    budget::{DecisionRow, SpendLedger},
    cache::Purpose,
};
use tkm_integrations::{
    Escalator,
    detect::{Mode, SystemClock},
    runtime::Integrations,
    superorchestrator::SpendReporter,
};
use tkm_proto::Host;
use tkmd::decider::StoreLedger;

#[test]
fn ledger_reports_only_uncached_known_costs_with_session_run() {
    let dir = tempfile::tempdir().unwrap();
    let bin = dir.path().join("superorchestrator");
    fs::write(&bin, "#!/bin/sh\nprintf '[]'\n").unwrap();
    fs::set_permissions(bin, fs::Permissions::from_mode(0o755)).unwrap();
    let integration = Arc::new(Integrations::new(
        Mode::On,
        Mode::Off,
        dir.path().as_os_str().to_owned(),
        dir.path().to_path_buf(),
        BTreeMap::new(),
        Arc::new(SystemClock),
    ));
    integration.detect_now(dir.path());
    let env = BTreeMap::from([("SUPERORCHESTRATOR_RUN_ID".into(), "run".into())]);
    integration.observe_session(Host::Claude, "session", dir.path(), &env);
    let path = dir.path().join("spend.jsonl");
    let ledger = StoreLedger::default();
    ledger.set_spend_sink(Arc::new(SpendReporter::new(path.clone())), integration);
    let mut row = DecisionRow {
        ts_ms: 0,
        session_id: "claude:session".into(),
        purpose: Purpose::LoopGuard,
        requested_model: "model".into(),
        model_version: None,
        provider: None,
        input_tokens: 1,
        output_tokens: 1,
        cost_usd: Some(0.1),
        latency_ms: 0,
        question_names: Vec::new(),
        outcome: "ok".into(),
        cached: false,
    };
    ledger.record(row.clone());
    row.cached = true;
    ledger.record(row.clone());
    row.cached = false;
    row.cost_usd = None;
    ledger.record(row);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    let text = loop {
        if let Ok(text) = fs::read_to_string(&path)
            && !text.is_empty()
        {
            break text;
        }
        assert!(std::time::Instant::now() < deadline);
        std::thread::sleep(std::time::Duration::from_millis(10));
    };
    assert_eq!(text.lines().count(), 1);
    let value: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(value["account"], "tokenimum-decisions");
    assert_eq!(value["run_id"], "run");
}

#[test]
fn slow_spend_sink_does_not_block_record() {
    struct Enabled;
    impl Escalator for Enabled {
        fn superorchestrator_enabled(&self) -> bool {
            true
        }
        fn handles(&self, _: Host, _: &str) -> bool {
            false
        }
        fn escalate(&self, _: &tkm_integrations::EscalationEvent) {}
    }
    struct Slow;
    impl tkm_integrations::superorchestrator::SpendSink for Slow {
        fn record(&self, _: &str, _: &str, _: f64, _: Option<&str>) {
            std::thread::sleep(std::time::Duration::from_millis(500));
        }
    }
    let ledger = StoreLedger::default();
    ledger.set_spend_sink(Arc::new(Slow), Arc::new(Enabled));
    let row = DecisionRow {
        ts_ms: 0,
        session_id: "claude:s".into(),
        purpose: Purpose::LoopGuard,
        requested_model: "model".into(),
        model_version: None,
        provider: None,
        input_tokens: 1,
        output_tokens: 1,
        cost_usd: Some(0.1),
        latency_ms: 0,
        question_names: Vec::new(),
        outcome: "ok".into(),
        cached: false,
    };
    let start = std::time::Instant::now();
    ledger.record(row);
    assert!(start.elapsed() < std::time::Duration::from_millis(250));
}
