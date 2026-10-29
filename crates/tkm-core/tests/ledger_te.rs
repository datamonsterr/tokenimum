#![forbid(unsafe_code)]
#![allow(clippy::float_cmp, clippy::unwrap_used, clippy::expect_used)]
use tkm_core::{
    ledger::{CostLedger, Overhead, effective_exec, overhead, overhead_from_json},
    model::Usage,
};
#[test]
fn te_ledger_overhead_sums_only_known_keys() {
    let x = Overhead {
        optimizer_tokens: 10.0,
        spiral_judge_tokens: 5.0,
        untool_judge_tokens: 3.0,
        ..Overhead::default()
    };
    assert_eq!(overhead(&x), 18.0);
}
#[test]
fn te_ledger_overhead_ignores_non_numeric() {
    assert_eq!(
        overhead_from_json(
            &serde_json::json!({"optimizer_tokens": null, "spiral_judge_tokens": "x", "unrelated": 999})
        ),
        0.0
    );
    assert_eq!(
        overhead_from_json(
            &serde_json::json!({"optimizer_tokens": 10, "spiral_judge_tokens": 5, "unrelated": 999})
        ),
        15.0
    );
}
#[test]
fn te_ledger_all_overhead_keys_counted() {
    let x = Overhead {
        optimizer_tokens: 1.0,
        untool_classify_tokens: 1.0,
        untool_judge_tokens: 1.0,
        codex_untool_adapt_tokens: 1.0,
        spiral_judge_tokens: 1.0,
        spiral_nudge_writer_tokens: 1.0,
    };
    assert_eq!(overhead(&x), 6.0);
}
#[test]
fn te_ledger_effective_exec_uses_breakdown() {
    assert_eq!(
        effective_exec(
            Some(Usage {
                input: 100,
                cache_read: 1000,
                output: 10,
                cache_write: 0
            }),
            0.0
        ),
        250.0
    );
}
#[test]
fn te_ledger_effective_exec_falls_back_to_total() {
    assert_eq!(effective_exec(None, 777.0), 777.0);
}
#[test]
fn te_ledger_cost_ledger_amortizes() {
    let mut l = CostLedger::default();
    l.add(100.0, 20.0).unwrap();
    l.add(200.0, 0.0).unwrap();
    assert_eq!(l.cum_exec, 300.0);
    assert_eq!(l.cum_overhead, 20.0);
    assert_eq!(l.cum_total(), Some(320.0));
    assert_eq!(l.amortized_per_task(), Some(160.0));
    assert!(l.overhead_frac().unwrap() > 0.0 && l.overhead_frac().unwrap() < 1.0);
}

#[test]
fn ledger_count_overflow_is_atomic() {
    let mut ledger = CostLedger {
        cum_exec: 1.0,
        cum_overhead: 2.0,
        n: u64::MAX,
    };
    assert_eq!(ledger.checked_add(3.0, 4.0), None);
    assert_eq!(
        (ledger.cum_exec, ledger.cum_overhead, ledger.n),
        (1.0, 2.0, u64::MAX)
    );
}
