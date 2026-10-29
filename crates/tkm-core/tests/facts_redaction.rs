#![allow(clippy::unwrap_used, clippy::expect_used, clippy::pedantic)]
//! Reporting redacts validated fact values before inserting them into trusted templates.
#![forbid(unsafe_code)]

use tkm_core::facts::{Candidate, Family, Params, Scope, render};

fn candidate(value: &str) -> Candidate {
    Candidate {
        id: "fixture".into(),
        family: Family::EnvValue,
        params: Params {
            variable: "OPENROUTER_API_KEY".into(),
            good: [(value.into(), 3)].into(),
            ..Params::default()
        },
        scope: Scope::Neutral,
        evidence: Vec::new(),
    }
}

#[test]
fn fact_redaction_keeps_template_and_allows_markers_after_validation() {
    let fact = candidate("sk-or-v1-fake-credential");
    assert_eq!(
        render::claim(&fact),
        render::claim_redacted(&fact, str::to_owned)
    );
    let rendered = render::claim_redacted(&fact, |value| {
        value.replace("sk-or-v1-fake-credential", "[redacted:openrouter]")
    });
    assert_eq!(
        rendered,
        "Code that imports the project needs `OPENROUTER_API_KEY` set here: `OPENROUTER_API_KEY=[redacted:env]` works."
    );
}

#[test]
fn fact_redaction_cannot_bypass_parameter_validation() {
    let fact = candidate("../private");
    assert_eq!(render::claim_redacted(&fact, |_| "safe".into()).len(), 0);
}

#[test]
fn sensitive_env_names_mask_good_and_bad_values_without_process_environment() {
    for name in [
        "DATABASE_PASSWORD",
        "SERVICE_PASSWD",
        "API_KEY",
        "ACCESS_TOKEN",
        "APP_SECRET",
        "SERVICE_CREDENTIAL",
        "HTTP_AUTH",
        "PRIVATE_SETTING",
    ] {
        let mut fact = candidate("fixture_good_value");
        fact.params.variable = name.into();
        fact.params.bad.insert("fixture_bad_value".into(), 1);
        for rendered in [
            render::claim(&fact),
            render::claim_redacted(&fact, str::to_owned),
        ] {
            assert!(rendered.contains(name), "{rendered}");
            assert!(!rendered.contains("fixture_good_value"), "{rendered}");
            assert!(!rendered.contains("fixture_bad_value"), "{rendered}");
            assert_eq!(rendered.matches("[redacted:env]").count(), 2, "{rendered}");
        }
    }
}

#[test]
fn ordinary_env_names_keep_good_and_bad_values() {
    let mut fact = candidate("test_settings");
    fact.params.variable = "DJANGO_SETTINGS_MODULE".into();
    fact.params.bad.insert("other_settings".into(), 1);
    let rendered = render::claim_redacted(&fact, str::to_owned);
    assert!(
        rendered.contains("DJANGO_SETTINGS_MODULE=test_settings"),
        "{rendered}"
    );
    assert!(
        rendered.contains("`other_settings` does not."),
        "{rendered}"
    );
}
