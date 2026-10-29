#![allow(clippy::unwrap_used, clippy::expect_used)]
use tkm_core::{
    ledger::{CostLedger, Overhead, effective_exec, overhead},
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
    assert_eq!(overhead(&Overhead::default()), 0.0);
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
    l.add(100.0, 20.0);
    l.add(200.0, 0.0);
    assert_eq!(l.cum_exec, 300.0);
    assert_eq!(l.cum_overhead, 20.0);
    assert_eq!(l.cum_total(), 320.0);
    assert_eq!(l.amortized_per_task(), 160.0);
    assert!(l.overhead_frac() > 0.0 && l.overhead_frac() < 1.0);
}
