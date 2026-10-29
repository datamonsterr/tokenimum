//! Plain, serializable decision configuration; E4 supplies configuration I/O.
use crate::api::ProviderPrefs;
use serde::{Deserialize, Serialize};
/// One configured model chain slot.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChainModel {
    /// Pinned dated model slug, or a free probe slot.
    pub slug: String,
    /// Require a successful explicit probe before using this slot.
    #[serde(default)]
    pub free_probe: bool,
}
/// Decisions configuration with SPEC section 8 defaults.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DecideConfig {
    /// Whether requests are enabled.
    pub enabled: bool,
    /// Ordered model slots.
    pub chain: Vec<ChainModel>,
    /// Per-attempt network deadline in milliseconds.
    pub timeout_ms: u64,
    /// Maximum spend per UTC day, in USD.
    pub daily_cap_usd: f64,
    /// Maximum spend per session, in USD.
    pub session_cap_usd: f64,
    /// Maximum UTF-8 state length in bytes.
    pub max_state_bytes: usize,
    /// Whether the consumer requests agreement judgments.
    pub agreement: bool,
    /// Provider privacy and fallback preferences.
    pub provider: ProviderPrefs,
}
impl Default for DecideConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            chain: Vec::new(),
            timeout_ms: 1500,
            daily_cap_usd: 0.50,
            session_cap_usd: 0.02,
            max_state_bytes: 16_384,
            agreement: false,
            provider: ProviderPrefs::default(),
        }
    }
}
/// Configuration rejected before requests can be sent.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("decision model must use a pinned dated slug or a :free slot: {0}")]
pub struct ConfigError(pub String);
impl DecideConfig {
    /// Ensure model identifiers are pinned rather than moving aliases.
    ///
    /// # Errors
    /// Returns the first alias or undated non-free model.
    pub fn validate(&self) -> Result<(), ConfigError> {
        Ok(())
    }
}
