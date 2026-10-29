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
        "Code that imports the project needs `OPENROUTER_API_KEY` set here: `OPENROUTER_API_KEY=[redacted:openrouter]` works."
    );
}

#[test]
fn fact_redaction_cannot_bypass_parameter_validation() {
    let fact = candidate("../private");
    assert!(render::claim_redacted(&fact, |_| "safe".into()).is_empty());
}
