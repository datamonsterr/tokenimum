// Portions ported from microsoft/TokenEconomizer (tokeneconomizer/microskills/distiller.py, commit d0675a3),
// Copyright (c) Microsoft Corporation, licensed under the Apache License, Version 2.0.
//! Context injection assembly.

use crate::model::Timestamp;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[path = "explain.rs"]
pub mod explain;

/// Candidate origin.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Source {
    /// Repository fact.
    Facts,
    /// Selected skill.
    Skills,
    /// Memory index pointer.
    Memory,
}
/// A pre-rendered candidate.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InjectCandidate {
    /// Stable candidate identifier.
    pub id: String,
    /// Origin.
    pub source: Source,
    /// Template output.
    pub text: String,
    /// Estimated token count.
    pub tokens: usize,
    /// Fact prevents a recurring failure.
    pub fire_gate_prevents: bool,
}
/// Selection limits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Config {
    /// Scored block token budget.
    pub max_tokens: usize,
    /// Fallback fact count budget.
    pub max_facts: usize,
    /// Fallback fact token budget.
    pub max_fact_tokens: usize,
    /// Include memory pointers in fallback.
    pub memory: bool,
}
impl Default for Config {
    fn default() -> Self {
        Self {
            max_tokens: 600,
            max_facts: 4,
            max_fact_tokens: 150,
            memory: false,
        }
    }
}
/// Estimate tokens.
#[must_use]
pub fn estimate_tokens(_text: &str) -> usize {
    0
}
/// Remove duplicates, preserving order.
#[must_use]
pub fn dedupe(_items: &[InjectCandidate]) -> Vec<InjectCandidate> {
    Vec::new()
}
/// Select candidates.
#[must_use]
pub fn select(
    _items: &[InjectCandidate],
    _scores: Option<&BTreeMap<String, f64>>,
    _config: Config,
) -> Vec<InjectCandidate> {
    Vec::new()
}
/// Render a context block.
#[must_use]
pub fn render(_items: &[InjectCandidate]) -> String {
    String::new()
}
/// Cache lookup key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct CacheKey {
    /// Prompt hash.
    pub prompt: u64,
    /// Candidate-set hash.
    pub candidates: u64,
}
/// Construct a stable cache key.
#[must_use]
pub fn cache_key(_prompt: &str, _items: &[InjectCandidate]) -> CacheKey {
    CacheKey {
        prompt: 0,
        candidates: 0,
    }
}
/// Pure cache of rendered blocks.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Cache;
impl Cache {
    /// Store a block at a caller-supplied time.
    pub fn insert(&mut self, _key: CacheKey, _now: Timestamp, _block: String) {}
    /// Return a block that has not expired.
    #[must_use]
    pub fn get(&self, _key: CacheKey, _now: Timestamp) -> Option<&str> {
        None
    }
    /// Cache a default block by candidate set.
    pub fn default_block(&mut self, _items: &[InjectCandidate], _config: Config) -> String {
        String::new()
    }
}
