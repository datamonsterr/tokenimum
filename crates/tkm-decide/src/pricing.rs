//! Offline model pricing conversion.
use serde::Deserialize;
use toml_edit::{DocumentMut, Item, Table, value};
/// Model discovery endpoint.
pub const MODELS_URL: &str = "https://openrouter.ai/api/v1/models";
/// One model's prices.
#[derive(Debug, Clone)]
pub struct ModelPrice {
    /// Public slug.
    pub slug: String,
    /// Canonical slug.
    pub canonical: String,
    /// Prompt price per token.
    pub prompt_per_token: f64,
    /// Completion price per token.
    pub completion_per_token: f64,
    /// Context length.
    pub context: u64,
}
/// Invalid model data or TOML.
#[derive(Debug, thiserror::Error)]
#[error("invalid pricing data: {0}")]
pub struct PricingError(pub String);
#[derive(Deserialize)]
struct Listing {
    data: Vec<Entry>,
}
#[derive(Deserialize)]
struct Entry {
    id: String,
    canonical_slug: String,
    pricing: Prices,
    context_length: u64,
}
#[derive(Deserialize)]
struct Prices {
    prompt: String,
    completion: String,
}
/// Parses model JSON, omitting alias slugs.
///
/// # Errors
/// Returns an error for malformed JSON or invalid prices.
pub fn parse_models(json: &str) -> Result<Vec<ModelPrice>, PricingError> {
    let listing: Listing = serde_json::from_str(json).map_err(|e| PricingError(e.to_string()))?;
    listing
        .data
        .into_iter()
        .filter(|entry| !entry.id.starts_with('~'))
        .map(|entry| {
            let parse = |s: &str| -> Result<f64, PricingError> {
                let price: f64 = s
                    .parse()
                    .map_err(|_| PricingError(format!("invalid price: {s}")))?;
                if !price.is_finite() || price < 0.0 {
                    return Err(PricingError(format!("invalid price: {s}")));
                }
                Ok(price)
            };
            Ok(ModelPrice {
                slug: entry.id,
                canonical: entry.canonical_slug,
                prompt_per_token: parse(&entry.pricing.prompt)?,
                completion_per_token: parse(&entry.pricing.completion)?,
                context: entry.context_length,
            })
        })
        .collect()
}
/// Updates model tables while retaining other document content.
///
/// # Errors
/// Returns an error for invalid TOML, conflicting tables, or oversized contexts.
pub fn render_pricing_toml(
    existing: &str,
    models: &[ModelPrice],
    fetched_date: &str,
) -> Result<String, PricingError> {
    let mut doc = existing
        .parse::<DocumentMut>()
        .map_err(|e| PricingError(e.to_string()))?;
    if !doc.contains_key("openrouter") {
        doc["openrouter"] = Item::Table(Table::new());
    }
    let root = doc["openrouter"]
        .as_table_mut()
        .ok_or_else(|| PricingError("openrouter is not a table".into()))?;
    for model in models {
        if !root.contains_key(&model.slug) {
            root[model.slug.as_str()] = Item::Table(Table::new());
        }
        let table = root[model.slug.as_str()]
            .as_table_mut()
            .ok_or_else(|| PricingError(format!("{} is not a table", model.slug)))?;
        table["prompt_per_token"] = value(model.prompt_per_token);
        table["completion_per_token"] = value(model.completion_per_token);
        table["canonical"] = value(model.canonical.as_str());
        table["context"] =
            value(i64::try_from(model.context).map_err(|e| PricingError(e.to_string()))?);
        table["fetched"] = value(fetched_date);
    }
    Ok(doc.to_string())
}
