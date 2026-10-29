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
