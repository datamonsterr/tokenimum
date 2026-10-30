//! Labelled, normalized fixture turns for facts and relevance calibration.
use super::{CalibrateArgs, CliError, Groups, labelled_samples, selected};
use serde::Deserialize;
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};
use tkm_core::{facts::FactsState, model::Session};
#[derive(Deserialize)]
struct File {
    source: String,
    turns: Vec<Turn>,
}
#[derive(Deserialize)]
struct Turn {
    session: Session,
    facts: FactsState,
    injected: BTreeSet<String>,
    expected: BTreeMap<String, bool>,
    judgments: Vec<Judgment>,
}
#[derive(Deserialize)]
struct Judgment {
    model: String,
    purpose: String,
    answers: serde_json::Value,
}
pub(super) fn load(root: &Path, args: &CalibrateArgs, groups: &mut Groups) -> Result<(), CliError> {
    if !["fact_filter", "relevance"]
        .iter()
        .any(|purpose| selected(args, purpose))
    {
        return Ok(());
    }
    let file: File = serde_json::from_slice(&std::fs::read(root.join("outcomes.json"))?)
        .map_err(|e| CliError::Usage(format!("invalid outcome fixtures: {e}")))?;
    if file.source != "synthetic test double" {
        return Err(CliError::Usage("unknown outcome fixture provenance".into()));
    }
    let mut identities = BTreeSet::new();
    for turn in file.turns {
        if turn.session.partial
            || !identities.insert((turn.session.host.as_str(), turn.session.session_id.clone()))
        {
            return Err(CliError::Usage(
                "partial or duplicate outcome fixture".into(),
            ));
        }
        let outcomes =
            tkm_decide::labels::session_outcomes(&turn.session, &turn.facts, &turn.injected);
        if outcomes != turn.expected {
            return Err(CliError::Usage(
                "outcome fixture labels do not match session evidence".into(),
            ));
        }
        for judgment in turn.judgments {
            if !selected(args, &judgment.purpose) {
                continue;
            }
            let Some(answers) = judgment.answers.as_object() else {
                return Err(CliError::Usage("invalid fixture answer batch".into()));
            };
            let labels =
                tkm_decide::labels::question_labels(answers.keys().map(String::as_str), &outcomes);
            let labels = serde_json::to_value(labels)
                .map_err(|e| CliError::Usage(format!("invalid fixture labels: {e}")))?;
            let samples = labelled_samples(&judgment.purpose, &judgment.answers, &labels);
            if samples.is_empty() {
                return Err(CliError::Usage(
                    "fixture judgment has no observed labels".into(),
                ));
            }
            groups
                .entry((judgment.model, judgment.purpose))
                .or_default()
                .extend(samples);
        }
    }
    Ok(())
}
