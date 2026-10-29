#![forbid(unsafe_code)]
#![allow(clippy::float_cmp, clippy::unwrap_used, clippy::expect_used)]
use tkm_core::{
    ledger::{DecisionOverhead, PriceTable, decision_overhead},
    model::{Host, Session, Usage},
};
#[test]
fn pricing_cache_classes_and_missing_models() {
    let prices = PriceTable::parse(include_str!("../../../fixtures/pricing/pricing.toml")).unwrap();
    for (usage, expected) in [
        (
            Usage {
                input: 1_000_000,
                ..Usage::default()
            },
            2.0,
        ),
        (
            Usage {
                output: 1_000_000,
                ..Usage::default()
            },
            10.0,
        ),
        (
            Usage {
                cache_read: 1_000_000,
                ..Usage::default()
            },
            0.2,
        ),
        (
            Usage {
                cache_write: 1_000_000,
                ..Usage::default()
            },
            2.5,
        ),
    ] {
        assert_eq!(prices.price_usage("example-model", usage), Some(expected));
    }
    assert_eq!(prices.price_usage("unknown", Usage::default()), None);
    let derived = PriceTable::parse("[models.claude_test]\ninput=2.0\noutput=10.0\n").unwrap();
    assert_eq!(
        derived.price_usage(
            "claude_test",
            Usage {
                cache_read: 1_000_000,
                cache_write: 1_000_000,
                ..Usage::default()
            }
        ),
        Some(2.7)
    );
}
#[test]
fn pricing_session_prefers_breakdown_and_requires_known_model() {
    let prices = PriceTable::parse(include_str!("../../../fixtures/pricing/pricing.toml")).unwrap();
    let mut session = Session::new(Host::Claude);
    session.usage.input = 1_000_000;
    assert_eq!(prices.price_session(&session, None), None);
    assert_eq!(
        prices.price_session(&session, Some("example-model")),
        Some(2.0)
    );
    session.usage_by_model.insert(
        "example-model".into(),
        Usage {
            output: 1_000_000,
            ..Usage::default()
        },
    );
    assert_eq!(prices.price_session(&session, None), Some(10.0));
    session
        .usage_by_model
        .insert("unknown".into(), Usage::default());
    assert_eq!(prices.price_session(&session, Some("example-model")), None);
}
#[test]
fn decision_costs_are_summed() {
    let ds: Vec<_> = [0.25, 0.75]
        .into_iter()
        .map(|cost| DecisionOverhead {
            input_tokens: 5,
            cost,
            model: "synthetic".into(),
            latency_ms: 10,
            question_names: vec!["question".into()],
        })
        .collect();
    assert_eq!(decision_overhead(&ds), 1.0);
    assert_eq!(decision_overhead(&[]), 0.0);
}

#[test]
fn decision_overhead_table() {
    for (costs, expected) in [
        (&[][..], 0.0),
        (&[0.25][..], 0.25),
        (&[0.25, 0.75][..], 1.0),
    ] {
        let decisions: Vec<_> = costs
            .iter()
            .map(|&cost| DecisionOverhead {
                input_tokens: 1,
                cost,
                model: "test".into(),
                latency_ms: 1,
                question_names: Vec::new(),
            })
            .collect();
        assert_eq!(decision_overhead(&decisions), expected);
    }
}
