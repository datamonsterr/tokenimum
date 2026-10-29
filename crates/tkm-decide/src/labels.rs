//! Outcome-derived labels shared by session folding and offline calibration.
//!
//! A batch is labelled by question key. Absence of exercise is unknown, rather than a
//! negative example. Contradiction questions have the opposite polarity to fact validity.
use std::collections::{BTreeMap, BTreeSet};
use tkm_core::{
    facts::{self, FactEventKind, FactsConfig, FactsState, Scope, SessionEvidence},
    model::Session,
};

/// Label a fact (or exercised context item) from independent session evidence.
#[must_use]
#[expect(
    clippy::fn_params_excessive_bools,
    reason = "Independent observed exercise, validity, contradiction and failure flags"
)]
pub const fn fact_outcome(
    exercised: bool,
    held: bool,
    contradiction: bool,
    failure: bool,
) -> Option<bool> {
    if !exercised {
        None
    } else if contradiction || failure {
        Some(false)
    } else if held {
        Some(true)
    } else {
        None
    }
}

/// Derive fact and invoked-skill outcomes from a complete, normalized session.
///
/// Code lifecycle verdicts establish confirmation/contradiction; a failed known working
/// form also contradicts immediately. Candidate extraction and judgement scores never
/// label themselves. Unknown tool outcomes and unexercised candidates stay unlabelled.
#[must_use]
pub fn session_outcomes(
    session: &Session,
    state: &FactsState,
    injected: &BTreeSet<String>,
) -> BTreeMap<String, bool> {
    if session.partial {
        return BTreeMap::new();
    }
    let input = SessionEvidence {
        session: session.clone(),
        injected_ids: injected.clone(),
        decisions: None,
    };
    let (_, events) = facts::update(state.clone(), &input, &FactsConfig::default());
    let mut labels = BTreeMap::new();
    for event in events {
        match event.kind {
            FactEventKind::Confirmed => {
                labels.entry(event.fact_id).or_insert(true);
            }
            FactEventKind::Contradicted => {
                labels.insert(event.fact_id, false);
            }
            _ => {}
        }
    }
    let observations = facts::observe::observe(session);
    for (id, fact) in &state.facts {
        if matches!(fact.candidate.scope, Scope::Host(host) if host != session.host) {
            continue;
        }
        if observations.observations.iter().any(|o| {
            fact.candidate.params.forms.contains_key(&o.form)
                && o.exit_code.is_some_and(|exit| exit != 0)
        }) {
            labels.insert(id.clone(), false);
        }
    }
    for call in session.turns.iter().flat_map(|turn| &turn.calls) {
        if !["Skill", "skill"].contains(&call.tool.as_str()) {
            continue;
        }
        let Ok(args) = serde_json::from_str::<serde_json::Value>(&call.args_digest) else {
            continue;
        };
        let Some(name) = args
            .get("skill")
            .and_then(serde_json::Value::as_str)
            .filter(|s| !s.trim().is_empty())
        else {
            continue;
        };
        if let Some(label) = fact_outcome(
            true,
            call.exit_code == Some(0),
            false,
            call.is_error || call.exit_code.is_some_and(|exit| exit != 0),
        ) {
            labels
                .entry(format!("skill:{name}"))
                .and_modify(|old| *old &= label)
                .or_insert(label);
        }
    }
    labels
}

/// Map candidate outcomes to the exact questions in a batch, omitting unknown items.
#[must_use]
pub fn question_labels<'a>(
    questions: impl IntoIterator<Item = &'a str>,
    outcomes: &BTreeMap<String, bool>,
) -> BTreeMap<String, bool> {
    questions
        .into_iter()
        .filter_map(|key| {
            let (id, invert) = if let Some(id) = key.strip_prefix("contradicts_") {
                (id, true)
            } else {
                (
                    ["stable_", "safe_", "rel_"]
                        .iter()
                        .find_map(|prefix| key.strip_prefix(*prefix))?,
                    false,
                )
            };
            outcomes
                .get(id)
                .map(|label| (key.to_owned(), *label ^ invert))
        })
        .collect()
}

/// Label a memory pointer only when the session actually read that exact reference.
/// Successful reads confirm relevance; observed read failures contradict it. A pointer
/// merely being injected, or an unrelated read, supplies no label.
#[must_use]
pub fn reference_outcome(session: &Session, reference: &str) -> Option<bool> {
    if session.partial || reference.is_empty() {
        return None;
    }
    let mut label = None;
    for call in session.turns.iter().flat_map(|turn| &turn.calls) {
        if !["Read", "read", "read_file"].contains(&call.tool.as_str()) {
            continue;
        }
        let Ok(args) = serde_json::from_str::<serde_json::Value>(&call.args_digest) else {
            continue;
        };
        let Some(path) = ["file_path", "path"]
            .iter()
            .find_map(|key| args.get(*key).and_then(serde_json::Value::as_str))
        else {
            continue;
        };
        if !std::path::Path::new(path).ends_with(reference) {
            continue;
        }
        if let Some(observed) = fact_outcome(
            true,
            call.exit_code == Some(0) || !call.output_digest.trim().is_empty(),
            false,
            call.is_error || call.exit_code.is_some_and(|exit| exit != 0),
        ) {
            label = Some(label.unwrap_or(true) && observed);
        }
    }
    label
}
