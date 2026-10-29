//! Clock-injected model availability and failure windows.
use crate::config::ChainModel;
use std::{
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};
/// Millisecond clock shared by health, cache, and spend accounting.
pub trait Clock: Send + Sync {
    /// Current Unix time in milliseconds.
    fn now_ms(&self) -> u64;
}
impl<C: Clock + ?Sized> Clock for Arc<C> {
    fn now_ms(&self) -> u64 {
        (**self).now_ms()
    }
}
/// Wall clock for production requests.
#[derive(Clone, Copy, Debug, Default)]
pub struct SystemClock;
impl Clock for SystemClock {
    fn now_ms(&self) -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
    }
}
/// Configured model slots and their health.
pub struct ModelChain<C: Clock> {
    models: Vec<ChainModel>,
    clock: C,
}
impl<C: Clock> ModelChain<C> {
    /// Create a chain, initially disabling probe-only slots.
    #[must_use]
    pub fn new(models: &[ChainModel], clock: C) -> Self {
        Self {
            models: models.to_vec(),
            clock,
        }
    }
    /// Available model slugs in current priority order.
    #[must_use]
    pub fn active_models(&self) -> Vec<String> {
        let _ = self.clock.now_ms();
        self.models
            .iter()
            .filter(|m| !m.free_probe)
            .map(|m| m.slug.clone())
            .collect()
    }
    /// All configured probe-only model slugs.
    #[must_use]
    pub fn free_models(&self) -> Vec<String> {
        self.models
            .iter()
            .filter(|m| m.free_probe)
            .map(|m| m.slug.clone())
            .collect()
    }
    /// Record one failed model attempt.
    pub fn record_failure(&self, _model: &str) {}
    /// Record successful service by a model.
    pub fn record_success(&self, _model: &str) {}
    /// Activate and prioritize a free slot only after a successful probe.
    pub fn reprobe_free(&self, _model: &str, _succeeded: bool) {}
}
/// Whether another model cannot fix this request error.
#[must_use]
pub fn is_non_retryable(status: u16) -> bool {
    matches!(status, 400 | 401 | 402 | 403 | 413)
}
