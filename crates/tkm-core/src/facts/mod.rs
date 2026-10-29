// Portions ported from microsoft/TokenEconomizer (tokeneconomizer/microskills/envfacts.py, commit d0675a3),
// Copyright (c) Microsoft Corporation, licensed under the Apache License, Version 2.0.
//! Pure repository-fact observation, extraction, lifecycle, and template rendering.
//! Readers supply normalized sessions; this module never reads logs, paths, or clocks.

use crate::model::{Host, Session};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub mod extract;
pub mod lifecycle;
pub mod lint;
pub mod observe;
pub mod render;

/// Defaults retained from SPEC 4.3 and TE lifecycle constants.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct FactsConfig {
    /// SPEC 4.3: three distinct confirming sessions promote a candidate.
    pub promote_confirmations: u32,
    /// SPEC 4.3: multiply the score by 0.8 for each finished session.
    pub recency_decay: f64,
    /// SPEC 4.3: contradictions weigh five confirmations.
    pub contradiction_weight: f64,
    /// SPEC 4.3: failure occurred or was prevented in at least 40% of exercises.
    pub fire_gate: f64,
    /// SPEC 4.3: observe four exercised sessions before injection.
    pub fire_min_sessions: u32,
    /// SPEC 4.3: at most four selected facts.
    pub max_facts: usize,
    /// SPEC 4.3: approximate token cap, four Unicode characters per token.
    pub max_tokens: usize,
}
impl Default for FactsConfig {
    fn default() -> Self {
        Self {
            promote_confirmations: 3,
            recency_decay: 0.8,
            contradiction_weight: 5.0,
            fire_gate: 0.40,
            fire_min_sessions: 4,
            max_facts: 4,
            max_tokens: 150,
        }
    }
}

/// Detector family; each has one code-owned rendering template.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Family {
    /// Failing command spelling followed by a working spelling.
    FormRepair,
    /// All fetches to a named host fail with a network error.
    Boundary,
    /// Discovery probes reveal no references beyond the checkout.
    EmptyProbe,
    /// A test label loads only after a structural rewrite.
    ArgumentRepair,
    /// A missing variable is satisfied by a later setting.
    EnvValue,
}
/// Scope of a fact, as defined in SPEC 7.1.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Scope {
    /// Portable command/environment evidence shared by harnesses.
    Neutral,
    /// Tool-name-specific evidence restricted to the originating harness.
    Host(Host),
}
/// Structural label repair, never prose supplied by a model.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum LabelRule {
    /// Replace double colons with dots.
    Separator,
    /// Remove this validated package or path prefix.
    Strip(String),
}
/// Validated family parameters and per-session evidence counts.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Params {
    /// Intent of a repaired form.
    pub intent: Option<observe::Intent>,
    /// Failing spelling for form repair.
    pub bad_form: String,
    /// Runner spelling for label repair.
    pub form: String,
    /// Number of sessions each alternative worked exactly as typed.
    pub forms: BTreeMap<String, u32>,
    /// Number of sessions each alternative failed without also working.
    pub form_failures: BTreeMap<String, u32>,
    /// Environmental error counts, restricted to a closed enum.
    pub errors: BTreeMap<observe::ErrorClass, u32>,
    /// Validated refused hosts and their occurrence counts.
    pub hosts: BTreeMap<String, u32>,
    /// Kinds of exercised discovery probes.
    pub probes: BTreeSet<observe::Probe>,
    /// Structural argument rewrite.
    pub rule: Option<LabelRule>,
    /// Validated failing-to-working label examples.
    pub examples: BTreeMap<String, String>,
    /// Uppercase environment variable name.
    pub variable: String,
    /// Successful setting values, using the restricted label grammar.
    pub good: BTreeMap<String, u32>,
    /// Unsuccessful setting values, using the restricted label grammar.
    pub bad: BTreeMap<String, u32>,
}
/// Source references for an extracted candidate.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Evidence {
    /// Normalized source session id.
    pub session_id: String,
    /// Zero-based call indices across all turns, including non-shell calls.
    pub call_indices: Vec<usize>,
}
/// A code-extracted fact; parameters are revalidated before rendering.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Candidate {
    /// Deterministic family and parameter identity.
    pub id: String,
    /// Detector family.
    pub family: Family,
    /// Restricted, non-prose template parameters.
    pub params: Params,
    /// Harness portability.
    pub scope: Scope,
    /// Source session and call references.
    pub evidence: Vec<Evidence>,
}
/// Persistable fact with its lifecycle record.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Fact {
    /// Extracted candidate and accumulated parameters.
    pub candidate: Candidate,
    /// Promotion, retirement, score and firing evidence.
    pub life: lifecycle::Lifecycle,
}
/// Repository-owned facts and replay protection.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct FactsState {
    /// Facts indexed by deterministic candidate id.
    pub facts: BTreeMap<String, Fact>,
    /// Session ids already folded; each session contributes once.
    pub processed: BTreeSet<String>,
}
/// Decision scores supplied externally; thresholds are calibrated by the caller.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct DecisionScores {
    /// Stability probability; absent scores use code-only promotion.
    pub stable: Option<f64>,
    /// Safety probability; absent scores use code-only promotion.
    pub safe: Option<f64>,
    /// Contradiction probability, used only for exercised facts.
    pub contradicts: Option<f64>,
}
/// Batched verdicts for SPEC 7.1; no requests are made by core.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Decisions {
    /// Calibrated lower threshold for stability.
    pub t_low: f64,
    /// Calibrated higher threshold for safety and contradiction.
    pub t_high: f64,
    /// Candidate-id keyed scores.
    pub scores: BTreeMap<String, DecisionScores>,
}
/// One finished session and the injection/verdict evidence supplied by the daemon.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionEvidence {
    /// Frozen normalized session model.
    pub session: Session,
    /// Fact ids actually injected for this session, including across reloads.
    pub injected_ids: BTreeSet<String>,
    /// None means decisions disabled or over budget: require one extra confirmation.
    pub decisions: Option<Decisions>,
}
/// Lifecycle event suitable for later explanation rendering.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FactEvent {
    /// Fact identity.
    pub fact_id: String,
    /// Source session.
    pub session_id: String,
    /// State/evidence transition.
    pub kind: FactEventKind,
}
/// Explanation category; text belongs to E2.9.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FactEventKind {
    /// A confirming session was recorded.
    Confirmed,
    /// A contradictory session was recorded.
    Contradicted,
    /// Candidate became active.
    Promoted,
    /// Candidate or active fact retired.
    Retired,
}

/// Folds a finished session without I/O. Partial and replayed sessions do not train facts.
///
/// SPEC 7.1: optional scores filter new candidates at `t_low`/`t_high`;
/// absent verdicts use code evidence and one extra promotion confirmation.
/// Code contradictions always count, independently of decision scores.
#[must_use]
pub fn update(
    mut state: FactsState,
    input: &SessionEvidence,
    cfg: &FactsConfig,
) -> (FactsState, Vec<FactEvent>) {
    if input.session.partial
        || state.processed.contains(&input.session.session_id)
        || !valid_config(cfg)
    {
        return (state, Vec::new());
    }
    let digest = observe::observe(&input.session);
    for candidate in extract::extract(&digest) {
        if !decision_allows(input, &candidate.id) {
            continue;
        }
        if let Some(existing) = state.facts.get_mut(&candidate.id) {
            extract::merge(&mut existing.candidate, candidate);
        } else {
            state.facts.insert(
                candidate.id.clone(),
                Fact {
                    candidate,
                    life: lifecycle::Lifecycle::default(),
                },
            );
        }
    }
    let mut events = Vec::new();
    for fact in state.facts.values_mut() {
        if matches!(fact.candidate.scope,Scope::Host(host) if host!=input.session.host) {
            continue;
        }
        let id = &fact.candidate.id;
        let score = input.decisions.as_ref().and_then(|d| d.scores.get(id));
        let code_only = !score.is_some_and(|s| {
            s.stable.is_some_and(f64::is_finite) && s.safe.is_some_and(f64::is_finite)
        }) || !decision_allows(input, id);
        let model_contradiction = input.decisions.as_ref().is_some_and(|d| {
            valid_thresholds(d)
                && score
                    .and_then(|s| s.contradicts)
                    .is_some_and(|s| s.is_finite() && s > d.t_high)
        });
        let injected = input.injected_ids.contains(&fact.candidate.id);
        let evidence = extract::exercise(fact, &digest, injected);
        events.extend(lifecycle::fold(
            fact,
            evidence,
            code_only,
            model_contradiction,
            input,
            cfg,
        ));
    }
    state.processed.insert(input.session.session_id.clone());
    (state, events)
}
fn valid_config(cfg: &FactsConfig) -> bool {
    cfg.promote_confirmations > 0
        && cfg.fire_min_sessions > 0
        && cfg.recency_decay.is_finite()
        && (0.0..=1.0).contains(&cfg.recency_decay)
        && cfg.contradiction_weight.is_finite()
        && cfg.contradiction_weight >= 0.0
        && cfg.fire_gate.is_finite()
        && (0.0..=1.0).contains(&cfg.fire_gate)
}
fn valid_thresholds(d: &Decisions) -> bool {
    d.t_low.is_finite()
        && d.t_high.is_finite()
        && (0.0..=1.0).contains(&d.t_low)
        && (d.t_low..=1.0).contains(&d.t_high)
}
fn decision_allows(input: &SessionEvidence, id: &str) -> bool {
    input.decisions.as_ref().is_none_or(|d| {
        !valid_thresholds(d)
            || d.scores.get(id).is_none_or(|s| {
                s.stable
                    .is_none_or(|v| v.is_finite() && (d.t_low..=1.0).contains(&v))
                    && s.safe
                        .is_none_or(|v| v.is_finite() && (d.t_high..=1.0).contains(&v))
            })
    })
}
