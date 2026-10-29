// Portions ported from microsoft/TokenEconomizer (tokeneconomizer/microskills/distiller.py, commit d0675a3),
// Copyright (c) Microsoft Corporation, licensed under the Apache License, Version 2.0.
// Portions ported from microsoft/TokenEconomizer (tokeneconomizer/microskills/envfacts.py, commit d0675a3),
// Copyright (c) Microsoft Corporation, licensed under the Apache License, Version 2.0.
// Portions ported from microsoft/TokenEconomizer (tokeneconomizer/microskills/stage.py, commit d0675a3),
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
    /// Maximum estimated tokens in the complete rendered block (SPEC 7.2).
    pub max_tokens: usize,
    /// Maximum fact count in either selection mode (SPEC 4.3).
    pub max_facts: usize,
    /// Maximum fact tokens in either selection mode (SPEC 4.3).
    pub max_fact_tokens: usize,
    /// Include memory pointers in either selection mode.
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
// Replace Unicode general categories Cc and Cf, including directional and
// zero-width formatting. A regex compilation failure fails closed.
pub(crate) fn sanitize_line(text: &str) -> String {
    let Ok(pattern) = regex::Regex::new(r"[\p{Cc}\p{Cf}]") else {
        return String::new();
    };
    pattern
        .replace_all(text, " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}
const HEADER: &str = "# Context\n";
const MIN_SCORE: f64 = 1.5;
const FIRE_GATE_SCORE: f64 = 1.0;
/// Select after filtering disabled sources and deduplication. Facts obey SPEC 4.3
/// caps in both modes. Scored candidates use SPEC 7.2 thresholds and order;
/// items exceeding fact caps are skipped, and the first exceeding the overall
/// budget ends selection. The complete rendered block, including its header and
/// line separators, is conservatively budgeted using the greater of each supplied
/// estimate and its rendered text estimate.
#[must_use]
pub fn select(
    items: &[InjectCandidate],
    scores: Option<&BTreeMap<String, f64>>,
    config: Config,
) -> Vec<InjectCandidate> {
    let eligible: Vec<_> = items
        .iter()
        .filter(|item| match item.source {
            Source::Facts => true,
            Source::Skills => scores.is_some(),
            Source::Memory => config.memory,
        })
        .cloned()
        .collect();
    let mut items = dedupe(&eligible);
    if let Some(scores) = scores {
        items.retain(|item| {
            scores.get(&item.id).is_some_and(|score| {
                score.is_finite()
                    && (0.0..=3.0).contains(score)
                    && *score
                        >= if item.source == Source::Facts && item.fire_gate_prevents {
                            FIRE_GATE_SCORE
                        } else {
                            MIN_SCORE
                        }
            })
        });
        items.sort_by(|a, b| {
            scores[&b.id]
                .total_cmp(&scores[&a.id])
                .then_with(|| a.id.cmp(&b.id))
        });
    }
    let mut facts = 0_usize;
    let mut fact_tokens = 0_usize;
    let mut text_tokens = 0_usize;
    let mut selected = Vec::new();
    for item in items {
        let tokens = item.tokens.max(estimate_tokens(&sanitize_line(&item.text)));
        if item.source == Source::Facts
            && (facts >= config.max_facts
                || fact_tokens
                    .saturating_add(tokens)
                    .saturating_add(facts.saturating_add(1).div_ceil(4))
                    > config.max_fact_tokens)
        {
            continue;
        }
        let next_text = text_tokens.saturating_add(tokens);
        let overhead = HEADER
            .chars()
            .count()
            .saturating_add(selected.len())
            .saturating_add(1)
            .div_ceil(4);
        if next_text.saturating_add(overhead) > config.max_tokens {
            break;
        }
        if item.source == Source::Facts {
            facts += 1;
            fact_tokens = fact_tokens.saturating_add(tokens);
        }
        text_tokens = next_text;
        selected.push(item);
    }
    selected
}
/// Render a single header followed by sanitized candidate text, or empty when
/// none exist. Unicode controls and format characters are replaced by spaces.
#[must_use]
pub fn render(items: &[InjectCandidate]) -> String {
    if items.is_empty() {
        return String::new();
    }
    let mut result = String::from(HEADER);
    for item in items {
        result.push_str(&sanitize_line(&item.text));
        result.push('\n');
    }
    result
}
/// Cache lookup key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct CacheKey {
    /// Prompt hash.
    pub prompt: u64,
    /// Hash of the ordered candidates and selection configuration.
    pub candidates: u64,
}
/// Construct a deterministic FNV-1a key over the prompt and ordered candidates.
/// All selection-relevant fields, limits, flags and fixed thresholds participate
/// in the candidate/configuration hash.
#[must_use]
pub fn cache_key(prompt: &str, items: &[InjectCandidate], config: Config) -> CacheKey {
    let mut bytes = Vec::new();
    for limit in [config.max_tokens, config.max_facts, config.max_fact_tokens] {
        bytes.extend_from_slice(&(limit as u64).to_le_bytes());
    }
    bytes.push(u8::from(config.memory));
    // Fixed relevance thresholds are part of the key's selection policy.
    for threshold in [MIN_SCORE, FIRE_GATE_SCORE] {
        bytes.extend_from_slice(&threshold.to_bits().to_le_bytes());
    }
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
/// At most 64 prompt entries are retained; insertion evicts the oldest timestamp.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Cache {
    entries: Vec<(CacheKey, Timestamp, String)>,
    default: Option<(u64, Config, String)>,
}
impl Cache {
    /// Store a block at a caller-supplied time.
    pub fn insert(&mut self, key: CacheKey, now: Timestamp, block: String) {
        self.prune(now);
        self.entries.retain(|(stored, _, _)| *stored != key);
        self.entries.push((key, now, block));
        self.prune(now);
    }
    fn prune(&mut self, now: Timestamp) {
        self.entries
            .retain(|(_, created, _)| now.0.saturating_sub(created.0) < 600_000);
        while self.entries.len() > 64 {
            if let Some((index, _)) = self
                .entries
                .iter()
                .enumerate()
                .min_by_key(|(_, (_, created, _))| *created)
            {
                self.entries.remove(index);
            }
        }
    }
    /// Prune expired entries and return a block younger than ten minutes;
    /// time moving backward is a miss.
    #[must_use]
    pub fn get(&mut self, key: CacheKey, now: Timestamp) -> Option<&str> {
        self.prune(now);
        let (_, created, block) = self.entries.iter().find(|(stored, _, _)| *stored == key)?;
        let age = now.0.checked_sub(created.0)?;
        (0..600_000).contains(&age).then_some(block.as_str())
    }
    /// Reuse the no-prompt fallback block while candidates and limits are unchanged.
    pub fn default_block(&mut self, items: &[InjectCandidate], config: Config) -> String {
        let key = cache_key("", items, config).candidates;
        if let Some((stored_key, stored_config, block)) = &self.default
            && *stored_key == key
            && *stored_config == config
        {
            return block.clone();
        }
        let block = render(&select(items, None, config));
        self.default = Some((key, config, block.clone()));
        block
    }
}
