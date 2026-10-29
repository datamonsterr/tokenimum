// Portions ported from microsoft/TokenEconomizer (tokeneconomizer/microskills/lifecycle.py, commit d0675a3),
// Copyright (c) Microsoft Corporation, licensed under the Apache License, Version 2.0.
//! Session-based promotion, recency score, retirement and firing gate.
use super::FactsConfig;
use serde::{Deserialize, Serialize};
/// Persisted lifecycle state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    /// Not yet confirmed enough times.
    #[default]
    Candidate,
    /// Confirmed, subject to the firing gate.
    Active,
    /// Permanently excluded.
    Retired,
}
/// Evidence counters independent of file-edit drift, as TE environment facts require.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Lifecycle {
    /// Current state.
    pub status: Status,
    /// Distinct confirming sessions.
    pub confirmations: u32,
    /// Sessions that contradicted the fact.
    pub contradictions: u32,
    /// Recency-weighted confirmation minus contradiction score.
    pub score: f64,
    /// Exercised sessions where failure occurred or was prevented.
    pub fire_num: u32,
    /// All exercised sessions.
    pub fire_den: u32,
    /// Consecutive exercised form contradictions (TE requires two).
    pub contradiction_streak: u32,
}
impl Lifecycle {
    /// ACTIVE and at least SPEC 4.3's four exercises with 40% firing.
    #[must_use]
    pub fn injectable(&self, cfg: &FactsConfig) -> bool {
        self.status == Status::Active
            && self.fire_den >= cfg.fire_min_sessions
            && self.fire_den > 0
            && f64::from(self.fire_num) / f64::from(self.fire_den) >= cfg.fire_gate
    }
}

pub(super) fn fold(
    fact: &mut super::Fact,
    mut evidence: super::extract::Exercise,
    code_only: bool,
    model_contradiction: bool,
    input: &super::SessionEvidence,
    cfg: &FactsConfig,
) -> Vec<super::FactEvent> {
    use super::extract::Verdict;
    use super::{FactEvent, FactEventKind, Family};
    let life = &mut fact.life;
    life.score *= cfg.recency_decay;
    if evidence.exercised {
        life.fire_den = life.fire_den.saturating_add(1);
        if evidence.fired {
            life.fire_num = life.fire_num.saturating_add(1);
        }
    }
    // TE envfacts.fold: one runner success can be a task with no missing imports;
    // require two consecutive exercised contradictions before recording one.
    if fact.candidate.family == Family::FormRepair && evidence.exercised {
        life.contradiction_streak = if evidence.verdict == Some(Verdict::Contradict) {
            life.contradiction_streak.saturating_add(1)
        } else {
            0
        };
        if evidence.verdict == Some(Verdict::Contradict) && life.contradiction_streak < 2 {
            evidence.verdict = None;
        }
    }
    if model_contradiction && evidence.exercised {
        evidence.verdict = Some(Verdict::Contradict);
    }
    let mut events = Vec::new();
    let event = |kind| FactEvent {
        fact_id: fact.candidate.id.clone(),
        session_id: input.session.session_id.clone(),
        kind,
    };
    match evidence.verdict {
        Some(Verdict::Confirm) => {
            life.confirmations = life.confirmations.saturating_add(1);
            life.score += 1.0;
            events.push(event(FactEventKind::Confirmed));
        }
        Some(Verdict::Contradict) => {
            life.contradictions = life.contradictions.saturating_add(1);
            life.score -= cfg.contradiction_weight;
            events.push(event(FactEventKind::Contradicted));
        }
        None => {}
    }
    let high = matches!(fact.candidate.family, Family::Boundary | Family::EmptyProbe);
    let hard = high && evidence.verdict == Some(Verdict::Contradict);
    if life.status != Status::Retired
        && (hard || (life.status == Status::Active && life.score < 0.0))
    {
        life.status = Status::Retired;
        events.push(event(FactEventKind::Retired));
    } else if life.status == Status::Candidate
        && life.confirmations
            >= cfg
                .promote_confirmations
                .saturating_add(u32::from(code_only))
    {
        life.status = Status::Active;
        events.push(event(FactEventKind::Promoted));
    }
    events
}
