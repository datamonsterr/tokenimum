#![forbid(unsafe_code)]
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::collections::BTreeMap;
use tkm_core::inject::{self, Cache, Config, InjectCandidate, Source};
use tkm_core::model::Timestamp;

fn item(id: &str, text: &str, source: Source, tokens: usize, gate: bool) -> InjectCandidate {
    InjectCandidate {
        id: id.into(),
        source,
        text: text.into(),
        tokens,
        fire_gate_prevents: gate,
    }
}
#[test]
fn dedupe_exact_normalized_and_jaccard_boundary() {
    let base = (0..100).map(|n| format!("w{n}")).collect::<Vec<_>>();
    for (shared, expected) in [(79, 2), (80, 1)] {
        let a = base.join(" ");
        let b = base[..shared].join(" ");
        let items = vec![
            item("a", &a, Source::Facts, 1, false),
            item("b", &b, Source::Skills, 1, false),
        ];
        assert_eq!(inject::dedupe(&items).len(), expected);
    }
    for second in ["Hello world", "HELLO,   WORLD!"] {
        let items = vec![
            item("a", "Hello world", Source::Facts, 1, false),
            item("b", second, Source::Skills, 1, false),
        ];
        assert_eq!(inject::dedupe(&items).len(), 1);
    }
}
#[test]
fn selection_thresholds_order_and_cap() {
    let items = vec![
        item("a", "alpha", Source::Facts, 3, false),
        item("b", "beta", Source::Facts, 2, true),
        item("c", "gamma", Source::Skills, 1, false),
        item("d", "delta", Source::Facts, 1, true),
    ];
    let scores = BTreeMap::from([
        ("a".into(), 1.5),
        ("b".into(), 1.0),
        ("c".into(), 1.49),
        ("d".into(), 0.99),
    ]);
    let selected = inject::select(
        &items,
        Some(&scores),
        Config {
            max_tokens: 4,
            ..Config::default()
        },
    );
    assert_eq!(
        selected.iter().map(|x| x.id.as_str()).collect::<Vec<_>>(),
        ["a"]
    );
    let selected = inject::select(&items, Some(&scores), Config::default());
    assert_eq!(
        selected.iter().map(|x| x.id.as_str()).collect::<Vec<_>>(),
        ["a", "b"]
    );
}
#[test]
fn te_injection_empty_render_before_formation() {
    assert_eq!(inject::render(&[]), "");
}
#[test]
fn te_injection_fallback_caps_and_memory() {
    let items = (0..6)
        .map(|n| {
            item(
                &n.to_string(),
                &format!("fact {n}"),
                Source::Facts,
                40,
                false,
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(inject::select(&items, None, Config::default()).len(), 3);
    assert_eq!(inject::estimate_tokens("12345"), 2);
}
#[test]
fn cache_ttl_and_candidate_change() {
    let items = vec![item("a", "alpha", Source::Facts, 1, false)];
    let key = inject::cache_key("prompt", &items);
    assert_ne!(key, inject::cache_key("other", &items));
    let mut cache = Cache::default();
    cache.insert(key, Timestamp(100), "block".into());
    assert_eq!(cache.get(key, Timestamp(600_099)), Some("block"));
    assert_eq!(cache.get(key, Timestamp(600_100)), None);
    assert_eq!(cache.get(key, Timestamp(99)), None);
    assert!(
        cache
            .default_block(&items, Config::default())
            .contains("alpha")
    );
}
