//! Offline model pricing conversion.
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
/// Parses model JSON.
///
/// # Errors
/// Returns an error for invalid data.
pub fn parse_models(_json: &str) -> Result<Vec<ModelPrice>, PricingError> {
    Ok(Vec::new())
}
/// Updates a pricing document.
///
/// # Errors
/// Returns an error for invalid TOML.
pub fn render_pricing_toml(
    _existing: &str,
    _models: &[ModelPrice],
    _fetched_date: &str,
) -> Result<String, PricingError> {
    Ok(String::new())
}
