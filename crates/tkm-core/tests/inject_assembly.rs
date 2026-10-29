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
            max_tokens: 7,
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

#[test]
fn fallback_does_not_let_disabled_sources_hide_facts() {
    let items = vec![
        item("skill", "same text", Source::Skills, 1, false),
        item("fact", "same text", Source::Facts, 1, false),
    ];
    assert_eq!(
        inject::select(&items, None, Config::default())[0].id,
        "fact"
    );
}

#[test]
fn render_removes_terminal_and_bidi_controls() {
    let dangerous = "safe\u{1b}[31m\0X\u{202a}A\u{202b}B\u{202c}C\u{202d}D\u{202e}E\u{2066}F\u{2067}G\u{2068}H\u{2069}I\u{0085}end";
    let rendered = inject::render(&[item("x", dangerous, Source::Facts, 1, false)]);
    for ch in rendered.chars() {
        assert!(
            ch == '\n'
                || (!ch.is_control()
                    && !matches!(ch, '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}'))
        );
    }
    assert_eq!(rendered.lines().count(), 2);
}
#[test]
fn scored_facts_obey_count_and_token_caps() {
    let items: Vec<_> = (0..5)
        .map(|i| {
            item(
                &format!("{i}"),
                &format!("unique{i}"),
                Source::Facts,
                30,
                false,
            )
        })
        .collect();
    let scores: BTreeMap<_, _> = items.iter().map(|i| (i.id.clone(), 2.0)).collect();
    assert_eq!(
        inject::select(&items, Some(&scores), Config::default()).len(),
        4
    );
    let config = Config {
        max_facts: 5,
        max_fact_tokens: 120,
        ..Config::default()
    };
    assert_eq!(inject::select(&items, Some(&scores), config).len(), 4);
    let config = Config {
        max_facts: 5,
        max_fact_tokens: 119,
        ..Config::default()
    };
    assert_eq!(inject::select(&items, Some(&scores), config).len(), 3);
}
#[test]
fn scored_disabled_memory_cannot_be_selected_or_hide_facts() {
    let items = vec![
        item("m", "same text", Source::Memory, 1, false),
        item("f", "same text", Source::Facts, 1, false),
        item("other", "another pointer", Source::Memory, 1, false),
    ];
    let scores = BTreeMap::from([("m".into(), 3.0), ("f".into(), 2.0), ("other".into(), 2.0)]);
    let selected = inject::select(&items, Some(&scores), Config::default());
    assert_eq!(
        selected.iter().map(|i| i.id.as_str()).collect::<Vec<_>>(),
        ["f"]
    );
}
#[test]
fn cache_prunes_expired_entries_on_lookup_and_insert_and_bounds_capacity() {
    let mut cache = Cache::default();
    for n in 0_u32..65 {
        cache.insert(
            inject::CacheKey {
                prompt: u64::from(n),
                candidates: 0,
            },
            Timestamp(i64::from(n)),
            format!("{n}"),
        );
    }
    assert_eq!(
        cache.get(
            inject::CacheKey {
                prompt: 0,
                candidates: 0
            },
            Timestamp(65)
        ),
        None
    );
    assert_eq!(
        cache.get(
            inject::CacheKey {
                prompt: 1,
                candidates: 0
            },
            Timestamp(65)
        ),
        Some("1")
    );
    let json = serde_json::to_value(&cache).unwrap();
    assert_eq!(json["entries"].as_array().unwrap().len(), 64);
    let _ = cache.get(
        inject::CacheKey {
            prompt: 64,
            candidates: 0,
        },
        Timestamp(600_064),
    );
    assert_eq!(
        serde_json::to_value(&cache).unwrap()["entries"]
            .as_array()
            .unwrap()
            .len(),
        0
    );
    cache.insert(
        inject::CacheKey {
            prompt: 100,
            candidates: 0,
        },
        Timestamp(1),
        "old".into(),
    );
    cache.insert(
        inject::CacheKey {
            prompt: 101,
            candidates: 0,
        },
        Timestamp(600_001),
        "new".into(),
    );
    assert_eq!(
        serde_json::to_value(&cache).unwrap()["entries"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
}
#[test]
fn rendered_budget_includes_header_and_separators_in_both_modes() {
    let items = vec![
        item("a", "12345678", Source::Facts, 2, false),
        item("b", "abcdefgh", Source::Facts, 2, false),
    ];
    let scores = BTreeMap::from([("a".into(), 2.0), ("b".into(), 2.0)]);
    for scores in [None, Some(&scores)] {
        for cap in 0..10 {
            let config = Config {
                max_tokens: cap,
                ..Config::default()
            };
            let selected = inject::select(&items, scores, config);
            assert!(
                inject::estimate_tokens(&inject::render(&selected)) <= cap,
                "cap {cap}"
            );
        }
    }
}
