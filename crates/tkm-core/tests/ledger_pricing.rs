#![forbid(unsafe_code)]
#![allow(clippy::float_cmp, clippy::unwrap_used, clippy::expect_used)]
use tkm_core::{
    ledger::{DecisionOverhead, PriceTable, checked_decision_overhead, decision_overhead},
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
    session.usage = Usage {
        output: 1_000_000,
        ..Usage::default()
    };
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
fn pricing_maps_harness_model_ids_to_provider_keys() {
    let prices = PriceTable::parse(
        r#"
[models."anthropic/claude-opus-5.5"]
input = 5.0
output = 25.0
[models."anthropic/claude-opus-5.5-20260921"]
input = 5.0
output = 25.0
[models."anthropic/claude-opus-5.5:batch"]
input = 2.5
output = 12.5
[models."anthropic/claude-3.5-sonnet"]
input = 3.0
output = 15.0
"#,
    )
    .unwrap();
    let usage = Usage {
        input: 1_000_000,
        output: 1_000_000,
        ..Usage::default()
    };
    for id in [
        "claude-opus-5-5",
        "claude-opus-5-5-20260921",
        "claude-opus-5.5",
        "claude-opus-5-5[1m]",
        "Claude-Opus-5-5",
        "anthropic/claude-opus-5-5",
        "claude-opus-5-5@20260921",
    ] {
        assert_eq!(prices.price_usage(id, usage), Some(30.0), "{id}");
    }
    assert_eq!(
        prices.price_usage("claude-3-5-sonnet-20241022", usage),
        Some(18.0)
    );
    assert_eq!(prices.price_usage("claude-opus-5-6", usage), None);
}
#[test]
fn pricing_session_ignores_claude_synthetic_placeholder() {
    let prices =
        PriceTable::parse("[models.\"anthropic/claude-opus-5.5\"]\ninput=1.0\noutput=1.0\n")
            .unwrap();
    let mut session = Session::new(Host::Claude);
    let usage = Usage {
        input: 1_000_000,
        ..Usage::default()
    };
    session.usage = usage;
    session
        .usage_by_model
        .insert("claude-opus-5-5".into(), usage);
    session
        .usage_by_model
        .insert("<synthetic>".into(), Usage::default());
    assert_eq!(prices.price_session(&session, None), Some(1.0));
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
    assert_eq!(decision_overhead(&ds), Some(1.0));
    assert_eq!(decision_overhead(&[]), Some(0.0));
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
        assert_eq!(decision_overhead(&decisions), Some(expected));
    }
}

#[test]
fn invalid_price_and_cost_boundaries() {
    for rate in ["nan", "inf", "-1.0"] {
        for key in ["input", "output", "cache_read", "cache_write"] {
            let mut entries = [
                "input=1.0".to_string(),
                "output=1.0".to_string(),
                "cache_read=1.0".to_string(),
                "cache_write=1.0".to_string(),
            ];
            let index = ["input", "output", "cache_read", "cache_write"]
                .iter()
                .position(|k| *k == key)
                .unwrap();
            entries[index] = format!("{key}={rate}");
            assert!(PriceTable::parse(&format!("[models.x]\n{}", entries.join("\n"))).is_err());
        }
    }
    for cost in [-1.0, f64::NAN, f64::INFINITY] {
        let d = DecisionOverhead {
            input_tokens: 0,
            cost,
            model: String::new(),
            latency_ms: 0,
            question_names: Vec::new(),
        };
        assert_eq!(checked_decision_overhead(&[d]), None);
    }
    let d = DecisionOverhead {
        input_tokens: 0,
        cost: f64::MAX,
        model: String::new(),
        latency_ms: 0,
        question_names: Vec::new(),
    };
    assert_eq!(checked_decision_overhead(&[d.clone(), d]), None);
    let table =
        PriceTable::parse("[models.x]\ninput=1e308\noutput=1e308\ncache_read=1.0\ncache_write=1.0")
            .unwrap();
    assert_eq!(
        table.price_usage(
            "x",
            Usage {
                input: u64::MAX,
                ..Usage::default()
            }
        ),
        None
    );
}

#[test]
fn undated_alias_wins_over_dated_snapshot_keys() {
    let table = PriceTable::parse(
        r#"
[models."anthropic/claude-opus-5.5-20260921"]
input = 2.0
output = 10.0
cache_read = 0.1
cache_write = 2.5
[models."anthropic/claude-opus-5.5"]
input = 4.0
output = 20.0
cache_read = 0.2
cache_write = 5.0
[models."anthropic/claude-opus-5.5:batch"]
input = 2.0
output = 10.0
cache_read = 0.1
cache_write = 2.5
"#,
    )
    .unwrap();
    let usage = Usage {
        input: 1_000_000,
        output: 1_000_000,
        ..Usage::default()
    };
    assert_eq!(table.price_usage("claude-opus-5-5", usage), Some(24.0));
    // Two dated snapshots with different prices and no alias stay ambiguous.
    let ambiguous = PriceTable::parse(
        "[models.\"a/m-1-20260101\"]\ninput=1.0\noutput=1.0\n[models.\"a/m-1-20260202\"]\ninput=2.0\noutput=2.0",
    )
    .unwrap();
    assert_eq!(ambiguous.price_usage("m-1", usage), None);
}
