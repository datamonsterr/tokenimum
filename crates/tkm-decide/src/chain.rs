//! Clock-injected model availability and failure windows.
use crate::config::ChainModel;
use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
    time::{SystemTime, UNIX_EPOCH},
};
const FAILURE_WINDOW_MS: u64 = 5 * 60 * 1000;
const SKIP_MS: u64 = 15 * 60 * 1000;
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
struct Slot {
    model: ChainModel,
    active: bool,
    promoted: bool,
    failures: VecDeque<u64>,
    skip_until_ms: u64,
}
impl Slot {
    fn probe_only(&self) -> bool {
        self.model.free_probe || self.model.slug.ends_with(":free")
    }
}
/// Configured model slots and their health.
#[allow(clippy::module_name_repetitions)] // Public names follow the decision-client contract.
pub struct ModelChain<C: Clock> {
    slots: Mutex<Vec<Slot>>,
    clock: C,
}
impl<C: Clock> ModelChain<C> {
    /// Create a chain, initially disabling probe-only slots.
    #[must_use]
    pub fn new(models: &[ChainModel], clock: C) -> Self {
        let slots = models
            .iter()
            .map(|model| Slot {
                active: !model.free_probe && !model.slug.ends_with(":free"),
                model: model.clone(),
                promoted: false,
                failures: VecDeque::new(),
                skip_until_ms: 0,
            })
            .collect();
        Self {
            slots: Mutex::new(slots),
            clock,
        }
    }
    /// Available model slugs in current priority order.
    #[must_use]
    pub fn active_models(&self) -> Vec<String> {
        let now = self.clock.now_ms();
        let slots = self
            .slots
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let available = |slot: &&Slot| slot.active && now >= slot.skip_until_ms;
        slots
            .iter()
            .filter(available)
            .filter(|s| s.promoted)
            .chain(slots.iter().filter(available).filter(|s| !s.promoted))
            .map(|s| s.model.slug.clone())
            .collect()
    }
    /// All configured probe-only model slugs.
    #[must_use]
    pub fn free_models(&self) -> Vec<String> {
        self.slots
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .iter()
            .filter(|s| s.probe_only())
            .map(|s| s.model.slug.clone())
            .collect()
    }
    /// Record one failed model attempt.
    pub fn record_failure(&self, model: &str) {
        let now = self.clock.now_ms();
        let mut slots = self
            .slots
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(slot) = slots.iter_mut().find(|s| s.model.slug == model) {
            slot.failures
                .retain(|ts| now.saturating_sub(*ts) <= FAILURE_WINDOW_MS);
            slot.failures.push_back(now);
            if slot.failures.len() >= 3 {
                slot.skip_until_ms = now.saturating_add(SKIP_MS);
                slot.failures.clear();
            }
        }
    }
    /// Record successful service without discarding failures still in the rolling window.
    pub fn record_success(&self, model: &str) {
        let now = self.clock.now_ms();
        let mut slots = self
            .slots
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(slot) = slots.iter_mut().find(|s| s.model.slug == model) {
            slot.failures
                .retain(|ts| now.saturating_sub(*ts) <= FAILURE_WINDOW_MS);
        }
    }
    /// Activate and prioritize a free slot only after a successful probe.
    pub fn reprobe_free(&self, model: &str, succeeded: bool) {
        let mut slots = self
            .slots
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(slot) = slots
            .iter_mut()
            .find(|s| s.model.slug == model && s.probe_only())
        {
            slot.active = succeeded;
            slot.promoted = succeeded;
            if succeeded {
                slot.failures.clear();
                slot.skip_until_ms = 0;
            }
        }
    }
}
/// Whether another model cannot fix this request error.
#[must_use]
pub fn is_non_retryable(status: u16) -> bool {
    matches!(status, 400 | 401 | 402 | 403 | 413)
}
