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
    /// Cache read price per token, when supplied.
    pub cache_read_per_token: Option<f64>,
    /// Cache write price per token, when supplied.
    pub cache_write_per_token: Option<f64>,
    /// Context length.
    pub context: u64,
}
/// Invalid model data or TOML.
#[derive(Debug, thiserror::Error)]
#[error("invalid pricing data: {0}")]
#[allow(clippy::module_name_repetitions)] // Public name identifies pricing failures.
pub struct PricingError(pub String);
#[derive(Deserialize)]
struct Listing {
    data: Vec<serde_json::Value>,
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
    input_cache_read: Option<String>,
    input_cache_write: Option<String>,
}
/// Parses model JSON, omitting alias slugs and malformed individual entries.
///
/// # Errors
/// Returns an error for malformed JSON or an invalid top-level listing.
pub fn parse_models(json: &str) -> Result<Vec<ModelPrice>, PricingError> {
    let listing: Listing = serde_json::from_str(json).map_err(|e| PricingError(e.to_string()))?;
    Ok(listing
        .data
        .into_iter()
        .filter_map(|entry| serde_json::from_value::<Entry>(entry).ok())
        .filter(|entry| !entry.id.starts_with('~'))
        .filter_map(|entry| {
            let parse = |s: &str| {
                s.parse::<f64>()
                    .ok()
                    .filter(|price| price.is_finite() && *price >= 0.0)
            };
            Some(ModelPrice {
                slug: entry.id,
                canonical: entry.canonical_slug,
                prompt_per_token: parse(&entry.pricing.prompt)?,
                completion_per_token: parse(&entry.pricing.completion)?,
                cache_read_per_token: entry.pricing.input_cache_read.as_deref().and_then(parse),
                cache_write_per_token: entry.pricing.input_cache_write.as_deref().and_then(parse),
                context: entry.context_length,
            })
        })
        .collect())
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
        update_value(table, "prompt_per_token", value(model.prompt_per_token));
        update_value(
            table,
            "completion_per_token",
            value(model.completion_per_token),
        );
        update_value(table, "canonical", value(model.canonical.as_str()));
        update_value(
            table,
            "context",
            value(i64::try_from(model.context).map_err(|e| PricingError(e.to_string()))?),
        );
        update_value(table, "fetched", value(fetched_date));
    }
    Ok(doc.to_string())
}

fn update_value(table: &mut Table, name: &str, mut new: Item) {
    if let Some(old) = table.get(name).and_then(Item::as_value)
        && let Some(new_value) = new.as_value_mut()
    {
        *new_value.decor_mut() = old.decor().clone();
    }
    table[name] = new;
}

/// Refresh the local model table when it is at least seven days old.
/// The injected fetch function makes network failure and time deterministic in tests.
///
/// # Errors
/// Returns an error on an invalid table, failed fetch, or invalid listing.
pub fn refresh_pricing(
    existing: &str,
    now_ms: i64,
    fetch: impl FnOnce() -> Result<String, String>,
) -> Result<String, PricingError> {
    let mut doc = existing
        .parse::<DocumentMut>()
        .map_err(|e| PricingError(e.to_string()))?;
    let fetched = doc.get("fetched_at").and_then(Item::as_integer);
    if fetched.is_some_and(|time| time <= now_ms && now_ms.saturating_sub(time) < 604_800_000) {
        return Ok(existing.to_owned());
    }
    let models = parse_models(&fetch().map_err(PricingError)?)?;
    if models.is_empty() {
        return Err(PricingError("model listing has no usable prices".into()));
    }
    if !doc.contains_key("models") {
        doc["models"] = Item::Table(Table::new());
    }
    let table = doc["models"]
        .as_table_mut()
        .ok_or_else(|| PricingError("models is not a table".into()))?;
    for model in models {
        let mut price = Table::new();
        price["input"] = value(model.prompt_per_token * 1_000_000.0);
        price["output"] = value(model.completion_per_token * 1_000_000.0);
        if let Some(rate) = model.cache_read_per_token {
            price["cache_read"] = value(rate * 1_000_000.0);
        }
        if let Some(rate) = model.cache_write_per_token {
            price["cache_write"] = value(rate * 1_000_000.0);
        }
        if model.canonical != model.slug {
            table[&model.canonical] = Item::Table(price.clone());
        }
        table[&model.slug] = Item::Table(price);
    }
    doc["fetched_at"] = value(now_ms);
    Ok(doc.to_string())
}

/// Refresh a pricing file from the model endpoint, retaining the old file on failure.
///
/// # Errors
/// Returns an error on network, parsing or filesystem failure.
pub async fn refresh_file(path: &std::path::Path, now_ms: i64) -> Result<(), PricingError> {
    let existing = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(error) => return Err(PricingError(error.to_string())),
    };
    let doc = existing
        .parse::<DocumentMut>()
        .map_err(|e| PricingError(e.to_string()))?;
    if doc
        .get("fetched_at")
        .and_then(Item::as_integer)
        .is_some_and(|time| time <= now_ms && now_ms.saturating_sub(time) < 604_800_000)
    {
        return Ok(());
    }
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|e| PricingError(e.to_string()))?;
    let body = client
        .get(MODELS_URL)
        .send()
        .await
        .and_then(reqwest::Response::error_for_status)
        .map_err(|e| PricingError(e.to_string()))?
        .text()
        .await
        .map_err(|e| PricingError(e.to_string()))?;
    let updated = refresh_pricing(&existing, now_ms, || Ok(body))?;
    let parent = path
        .parent()
        .ok_or_else(|| PricingError("pricing path has no parent".into()))?;
    std::fs::create_dir_all(parent).map_err(|e| PricingError(e.to_string()))?;
    let temp = path.with_extension(format!("toml.{}.tmp", std::process::id()));
    std::fs::write(&temp, updated).map_err(|e| PricingError(e.to_string()))?;
    std::fs::rename(&temp, path).map_err(|e| PricingError(e.to_string()))?;
    Ok(())
}
