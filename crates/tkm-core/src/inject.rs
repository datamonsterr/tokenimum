// Portions ported from microsoft/TokenEconomizer (tokeneconomizer/microskills/distiller.py, commit d0675a3),
// Copyright (c) Microsoft Corporation, licensed under the Apache License, Version 2.0.
//! Context injection assembly.

use crate::model::Timestamp;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

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
/// Estimates one token for each four Unicode scalar values, rounded up.
#[must_use]
pub fn estimate_tokens(text: &str) -> usize {
    text.chars().count().div_ceil(4)
}

// Strip ASCII punctuation, lowercase Unicode, and collapse whitespace. Punctuation
// is discarded rather than replaced, so identifiers such as foo-bar normalize
// to foobar. Non-ASCII punctuation remains significant.
fn normalize(text: &str) -> String {
    text.chars()
        .filter(|c| !c.is_ascii_punctuation())
        .flat_map(char::to_lowercase)
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}
fn hash(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325_u64, |h, b| {
        (h ^ u64::from(*b)).wrapping_mul(0x0000_0100_0000_01b3)
    })
}
/// Remove normalized exact and token Jaccard duplicates, keeping the earlier item.
/// Jaccard uses sets of normalized whitespace-separated words and threshold 0.8.
#[must_use]
pub fn dedupe(items: &[InjectCandidate]) -> Vec<InjectCandidate> {
    let mut kept: Vec<(String, u64, BTreeSet<String>)> = Vec::new();
    let mut result = Vec::new();
    for item in items {
        let normalized = normalize(&item.text);
        let hashed = hash(normalized.as_bytes());
        let words: BTreeSet<String> = normalized.split_whitespace().map(str::to_owned).collect();
        let duplicate = kept
            .iter()
            .any(|(previous, previous_hash, previous_words)| {
                if hashed == *previous_hash && normalized == *previous {
                    return true;
                }
                let union = words.union(previous_words).count();
                union > 0
                    && words.intersection(previous_words).count().saturating_mul(5)
                        >= union.saturating_mul(4)
            });
        if !duplicate {
            kept.push((normalized, hashed, words));
            result.push(item.clone());
        }
    }
    result
}
/// Select after deduplication. SPEC 7.2 scores use 1.5, or 1.0 for
/// preventing facts, and stop on the first item that exceeds the budget.
/// Fallback observes SPEC 4.3 fact caps and optionally includes memory pointers.
#[must_use]
pub fn select(
    items: &[InjectCandidate],
    scores: Option<&BTreeMap<String, f64>>,
    config: Config,
) -> Vec<InjectCandidate> {
    let mut items = dedupe(items);
    if let Some(scores) = scores {
        items.retain(|item| {
            scores.get(&item.id).is_some_and(|score| {
                score.is_finite()
                    && (0.0..=3.0).contains(score)
                    && *score
                        >= if item.source == Source::Facts && item.fire_gate_prevents {
                            1.0
                        } else {
                            1.5
                        }
            })
        });
        items.sort_by(|a, b| {
            scores[&b.id]
                .total_cmp(&scores[&a.id])
                .then_with(|| a.id.cmp(&b.id))
        });
        let mut used = 0_usize;
        items
            .into_iter()
            .take_while(|item| {
                if item.tokens > config.max_tokens.saturating_sub(used) {
                    false
                } else {
                    used += item.tokens;
                    true
                }
            })
            .collect()
    } else {
        let mut facts = 0_usize;
        let mut fact_tokens = 0_usize;
        let mut total = 0_usize;
        let mut selected = Vec::new();
        for item in items {
            match item.source {
                Source::Facts => {
                    if facts >= config.max_facts
                        || item.tokens > config.max_fact_tokens.saturating_sub(fact_tokens)
                    {
                        continue;
                    }
                    if item.tokens > config.max_tokens.saturating_sub(total) {
                        break;
                    }
                    facts += 1;
                    fact_tokens += item.tokens;
                    total += item.tokens;
                    selected.push(item);
                }
                Source::Memory if config.memory => {
                    if item.tokens > config.max_tokens.saturating_sub(total) {
                        break;
                    }
                    total += item.tokens;
                    selected.push(item);
                }
                Source::Memory | Source::Skills => {}
            }
        }
        selected
    }
}
/// Render a single header followed by candidate text, or empty when none exist.
#[must_use]
pub fn render(items: &[InjectCandidate]) -> String {
    if items.is_empty() {
        return String::new();
    }
    let mut result = String::from("# Context\n");
    for item in items {
        result.push_str(&item.text.split_whitespace().collect::<Vec<_>>().join(" "));
        result.push('\n');
    }
    result
}
/// Cache lookup key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct CacheKey {
    /// Prompt hash.
    pub prompt: u64,
    /// Candidate-set hash.
    pub candidates: u64,
}
/// Construct a deterministic FNV-1a key over the prompt and ordered candidates.
/// All selection-relevant fields participate in the candidate hash.
#[must_use]
pub fn cache_key(prompt: &str, items: &[InjectCandidate]) -> CacheKey {
    let mut bytes = Vec::new();
    for item in items {
        for field in [&item.id, &item.text] {
            bytes.extend_from_slice(&(field.len() as u64).to_le_bytes());
            bytes.extend_from_slice(field.as_bytes());
        }
        bytes.push(match item.source {
            Source::Facts => 0,
            Source::Skills => 1,
            Source::Memory => 2,
        });
        bytes.extend_from_slice(&(item.tokens as u64).to_le_bytes());
        bytes.push(u8::from(item.fire_gate_prevents));
    }
    CacheKey {
        prompt: hash(prompt.as_bytes()),
        candidates: hash(&bytes),
    }
}
/// Pure cache of rendered blocks. TTL is ten minutes (SPEC 7.2).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Cache {
    entries: Vec<(CacheKey, Timestamp, String)>,
    default: Option<(u64, Config, String)>,
}
impl Cache {
    /// Store a block at a caller-supplied time.
    pub fn insert(&mut self, key: CacheKey, now: Timestamp, block: String) {
        self.entries.retain(|(stored, _, _)| *stored != key);
        self.entries.push((key, now, block));
    }
    /// Return a block younger than ten minutes; time moving backward is a miss.
    #[must_use]
    pub fn get(&self, key: CacheKey, now: Timestamp) -> Option<&str> {
        let (_, created, block) = self.entries.iter().find(|(stored, _, _)| *stored == key)?;
        let age = now.0.checked_sub(created.0)?;
        (0..600_000).contains(&age).then_some(block.as_str())
    }
    /// Reuse the no-prompt fallback block while candidates and limits are unchanged.
    pub fn default_block(&mut self, items: &[InjectCandidate], config: Config) -> String {
        let key = cache_key("", items).candidates;
        if let Some((stored_key, stored_config, block)) = &self.default {
            if *stored_key == key && *stored_config == config {
                return block.clone();
            }
        }
        let block = render(&select(items, None, config));
        self.default = Some((key, config, block.clone()));
        block
    }
}
