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

#[test]
fn fallback_memory_and_fact_limits() {
    let mut items = (0..5)
        .map(|n| {
            item(
                &n.to_string(),
                &format!("fact {n}"),
                Source::Facts,
                1,
                false,
            )
        })
        .collect::<Vec<_>>();
    items.push(item("s", "skill", Source::Skills, 1, false));
    items.push(item("m", "memory", Source::Memory, 1, false));
    let selected = inject::select(
        &items,
        None,
        Config {
            memory: true,
            ..Config::default()
        },
    );
    assert_eq!(
        selected
            .iter()
            .map(|item| item.id.as_str())
            .collect::<Vec<_>>(),
        ["0", "1", "2", "3", "m"]
    );
}

#[test]
fn cache_roundtrip_and_default_invalidation() {
    let mut cache = Cache::default();
    let mut items = vec![item("a", "alpha", Source::Facts, 1, false)];
    let key = inject::cache_key("prompt", &items);
    cache.insert(key, Timestamp(10), "stored".into());
    assert!(
        cache
            .default_block(&items, Config::default())
            .contains("alpha")
    );
    let json = serde_json::to_string(&cache).unwrap();
    let mut cache: Cache = serde_json::from_str(&json).unwrap();
    assert_eq!(cache.get(key, Timestamp(11)), Some("stored"));
    items[0].text = "beta".into();
    assert_ne!(key, inject::cache_key("prompt", &items));
    assert!(
        cache
            .default_block(&items, Config::default())
            .contains("beta")
    );
}

#[test]
fn score_ties_sort_by_id_and_invalid_scores_are_ignored() {
    let items = vec![
        item("b", "beta", Source::Facts, 1, false),
        item("a", "alpha", Source::Skills, 1, false),
        item("c", "gamma", Source::Facts, 1, false),
    ];
    let scores = BTreeMap::from([("a".into(), 2.0), ("b".into(), 2.0), ("c".into(), f64::NAN)]);
    assert_eq!(
        inject::select(&items, Some(&scores), Config::default())
            .iter()
            .map(|item| item.id.as_str())
            .collect::<Vec<_>>(),
        ["a", "b"]
    );
}
