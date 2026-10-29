#![forbid(unsafe_code)]
#![allow(clippy::unwrap_used, clippy::expect_used)]
use tkm_decide::pricing::{parse_models, render_pricing_toml};
#[test]
fn snapshot_and_toml_preservation() {
    let models = parse_models(include_str!("fixtures/models-decisions-2026-10-29.json")).unwrap();
    let mercury = models
        .iter()
        .find(|m| m.slug == "inception/mercury-decide")
        .unwrap();
    assert!((mercury.prompt_per_token - 2e-8).abs() < 1e-15);
    assert_eq!(mercury.canonical, "inception/mercury-decide-20260930");
    assert!(models.iter().all(|m| !m.slug.starts_with('~')));
    let old = "# keep this\n[vendor.anthropic]\nprice = 3\n[openrouter.\"inception/mercury-decide\"]\nprompt_per_token = 99.0\n";
    let output = render_pricing_toml(old, &models, "2026-10-29").unwrap();
    assert!(
        output.contains("# keep this")
            && output.contains("[vendor.anthropic]")
            && output.contains("2026-10-29")
    );
    assert!(!output.contains("99.0"));
}

#[test]
fn malformed_model_entries_are_skipped_without_losing_valid_prices() {
    let models = parse_models(include_str!("fixtures/models-malformed-entries.json")).unwrap();
    assert_eq!(models.len(), 1);
    assert_eq!(models[0].slug, "valid-model");
    assert_eq!(models[0].canonical, "valid-model-20261029");
    assert_eq!(models[0].context, 65_536);
    assert!((models[0].prompt_per_token - 2e-8).abs() < f64::EPSILON);
}

#[test]
fn invalid_top_level_model_listing_returns_an_error() {
    for json in [
        "not json",
        "{}",
        "null",
        "[]",
        r#"{"data":null}"#,
        r#"{"data":{}}"#,
    ] {
        assert!(parse_models(json).is_err(), "accepted {json}");
    }
    assert_eq!(parse_models(r#"{"data":[]}"#).unwrap().len(), 0);
}

#[test]
fn updating_pricing_preserves_trailing_comments_on_every_existing_value() {
    let models = parse_models(include_str!("fixtures/models-malformed-entries.json")).unwrap();
    let output = render_pricing_toml(
        include_str!("fixtures/pricing-comments.toml"),
        &models,
        "2026-10-29",
    )
    .unwrap();
    for comment in [
        "document",
        "prompt",
        "completion",
        "canonical",
        "context",
        "fetched",
    ] {
        assert!(
            output.contains(&format!("{comment} comment")),
            "lost {comment}"
        );
    }
    let doc = output.parse::<toml_edit::DocumentMut>().unwrap();
    let model = &doc["openrouter"]["valid-model"];
    assert!(
        (model["prompt_per_token"].as_float().unwrap() - models[0].prompt_per_token).abs()
            < f64::EPSILON
    );
    assert_eq!(model["canonical"].as_str(), Some("valid-model-20261029"));
    assert_eq!(model["context"].as_integer(), Some(65_536));
    assert_eq!(model["fetched"].as_str(), Some("2026-10-29"));
}
