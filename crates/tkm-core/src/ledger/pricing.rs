// Portions ported from microsoft/TokenEconomizer (tokeneconomizer/pricing.py, commit d0675a3),
// Copyright (c) Microsoft Corporation, licensed under the Apache License, Version 2.0.
use super::valid_cost;
use crate::model::{Session, Usage};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
/// Prices in US dollars per million tokens. Missing cache rates use TokenEconomizer model-specific multipliers. An unknown
/// model with missing cache rates cannot be priced.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Price {
    /// Fresh input rate.
    pub input: f64,
    /// Output rate.
    pub output: f64,
    /// Cache-read rate, if explicitly specified.
    pub cache_read: Option<f64>,
    /// Cache-write rate, if explicitly specified.
    pub cache_write: Option<f64>,
}
/// Price table keyed by exact billed model ID under a `[models]` TOML table.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct PriceTable {
    /// Model-specific prices.
    pub models: BTreeMap<String, Price>,
}
/// Invalid pricing TOML or an invalid rate.
#[derive(Debug, thiserror::Error)]
pub enum PriceError {
    /// TOML syntax or schema error.
    #[error("invalid pricing TOML: {0}")]
    Parse(#[from] toml::de::Error),
    /// A model rate is negative or non-finite.
    #[error("invalid price for model {0}")]
    InvalidRate(String),
}

impl Price {
    fn valid(&self) -> bool {
        valid_cost(self.input)
            && valid_cost(self.output)
            && self.cache_read.is_none_or(valid_cost)
            && self.cache_write.is_none_or(valid_cost)
    }
}
impl PriceTable {
    /// Parse prices from supplied TOML text.
    ///
    /// # Errors
    /// Returns a parsing error for invalid TOML or a mismatched schema.
    pub fn parse(text: &str) -> Result<Self, PriceError> {
        let table: Self = toml::from_str(text)?;
        for (model, price) in &table.models {
            if !price.valid() {
                return Err(PriceError::InvalidRate(model.clone()));
            }
        }
        Ok(table)
    }
    /// Price a usage breakdown, returning `None` for an unknown model.
    #[must_use]
    #[allow(clippy::cast_precision_loss)]
    pub fn price_usage(&self, model: &str, u: Usage) -> Option<f64> {
        let p = self.models.get(model).or_else(|| {
            let mut matches = self
                .models
                .iter()
                .filter(|(id, _)| id.rsplit('/').next() == Some(model));
            let (_, price) = matches.next()?;
            if matches.next().is_some() {
                None
            } else {
                Some(price)
            }
        })?;
        if !p.valid() {
            return None;
        }
        let ratios = cache_ratios(model);
        let read = if u.cache_read == 0 {
            0.0
        } else {
            p.cache_read.or_else(|| ratios.map(|r| p.input * r.0))?
        };
        let write = if u.cache_write == 0 {
            0.0
        } else {
            p.cache_write.or_else(|| ratios.map(|r| p.input * r.1))?
        };
        let cost = (u.input as f64 * p.input
            + u.output as f64 * p.output
            + u.cache_read as f64 * read
            + u.cache_write as f64 * write)
            / 1_000_000.0;
        valid_cost(cost).then_some(cost)
    }
    /// Price a session. The caller supplies a session model when aggregate usage lacks
    /// per-model data; an unknown or absent model yields `None`.
    #[must_use]
    pub fn price_session(&self, session: &Session, model: Option<&str>) -> Option<f64> {
        if session.usage_by_model.is_empty() {
            self.price_usage(model?, session.usage)
        } else {
            let attributed = session
                .usage_by_model
                .values()
                .copied()
                .fold(Usage::default(), Usage::add);
            if attributed != session.usage {
                return None;
            }
            session
                .usage_by_model
                .iter()
                .try_fold(0.0, |sum, (id, usage)| {
                    let total = sum + self.price_usage(id, *usage)?;
                    valid_cost(total).then_some(total)
                })
        }
    }
}

// Ratios from TokenEconomizer's exact billed-model price snapshot.
fn cache_ratios(model: &str) -> Option<(f64, f64)> {
    let id = model.to_ascii_lowercase();
    let tail = id.rsplit('/').next()?;
    if tail.contains("claude") {
        return Some((0.1, 1.25));
    }
    match tail {
        "gpt-5.6-sol" | "gpt-5.6-luna" => Some((0.1, 1.25)),
        "gpt-5.6-terra" => Some((0.1, 1.248)),
        "gpt-5.3-codex" => Some((0.0971, 0.0)),
        "gemini-2.5-pro" => Some((0.096, 0.0)),
        "gpt-5-mini" => Some((0.08, 0.0)),
        "gpt-5.4-mini" | "mai-code-1-flash-picker" => Some((0.0933, 0.0)),
        "gemini-3-flash-preview"
        | "gemini-3.1-pro-preview"
        | "gemini-3.5-flash"
        | "gemini-3.6-flash"
        | "gpt-5.4"
        | "gpt-5.5" => Some((0.1, 0.0)),
        "gpt-5" | "gpt-5.1" | "gpt-5-codex" | "gpt-5.1-codex" | "gpt-5.1-codex-max"
        | "gpt-5.2-codex" => Some((0.1, 1.0)),
        "codex-mini-latest" => Some((0.25, 1.0)),
        _ => None,
    }
}
