#![forbid(unsafe_code)]
#![allow(clippy::unwrap_used, clippy::expect_used)]
use tkm_decide::{
    budget::{Budget, BudgetVerdict, DecisionRow, InMemoryLedger, SpendLedger},
    cache::Purpose,
};
fn row(ts_ms: u64, session: &str, cost: Option<f64>) -> DecisionRow {
    DecisionRow {
        ts_ms,
        session_id: session.into(),
        purpose: Purpose::LoopGuard,
        requested_model: "a-20261029".into(),
        model_version: None,
        provider: None,
        input_tokens: 1,
        output_tokens: 0,
        cost_usd: cost,
        latency_ms: 0,
        question_names: vec!["stuck".into()],
        outcome: "ok".into(),
        cached: false,
    }
}
#[test]
fn spending_equal_to_daily_or_session_cap_is_over_budget() {
    let ledger = InMemoryLedger::default();
    ledger.record(row(0, "session", Some(0.50)));
    assert_eq!(
        Budget::new(0.50, 1.0).check(&ledger, 0, "session"),
        BudgetVerdict::OverDaily
    );
    assert_eq!(
        Budget::new(1.0, 0.50).check(&ledger, 0, "session"),
        BudgetVerdict::OverSession
    );
    assert_eq!(
        Budget::new(1.0, 1.0).check(&ledger, 0, "session"),
        BudgetVerdict::Allowed
    );
}
#[test]
fn ledger_uses_utc_day_and_full_session_id_and_ignores_unknown_cost() {
    let ledger = InMemoryLedger::default();
    ledger.record(row(0, "first", Some(0.01)));
    ledger.record(row(86_400_000, "second", Some(0.02)));
    ledger.record(row(86_400_001, "first", None));
    assert!((ledger.spent_today_usd(86_400_001) - 0.02).abs() < f64::EPSILON);
    assert!((ledger.spent_session_usd("first") - 0.01).abs() < f64::EPSILON);
    assert_eq!(ledger.rows().len(), 3);
}
#[test]
fn invalid_costs_do_not_reduce_or_poison_spend() {
    let ledger = InMemoryLedger::default();
    for cost in [0.01, -1.0, f64::NAN, f64::INFINITY] {
        ledger.record(row(0, "s", Some(cost)));
    }
    assert!((ledger.spent_today_usd(0) - 0.01).abs() < f64::EPSILON);
}
