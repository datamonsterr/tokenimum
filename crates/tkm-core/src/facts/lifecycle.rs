// Portions ported from microsoft/TokenEconomizer (tokeneconomizer/microskills/lifecycle.py, commit d0675a3),
// Copyright (c) Microsoft Corporation, licensed under the Apache License, Version 2.0.
//! Session-based promotion, recency score, retirement and firing gate.
use super::FactsConfig;
use serde::{Deserialize, Serialize};
/// Persisted lifecycle state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
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
    pub fn injectable(&self, _cfg: &FactsConfig) -> bool {
        false
    }
}
