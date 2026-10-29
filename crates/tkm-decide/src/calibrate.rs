//! Calibration thresholds and shadow policy.
use std::collections::HashMap;
use std::sync::Mutex;
/// Decision thresholds.
#[derive(Debug, Clone, Copy, PartialEq)]
#[allow(clippy::struct_field_names)] // Threshold names match SPEC section 8.5.
pub struct Thresholds {
    /// Stuck threshold.
    pub t_stuck: f64,
    /// Progress threshold.
    pub t_progress: f64,
    /// Low threshold.
    pub t_low: f64,
    /// High threshold.
    pub t_high: f64,
}
impl Default for Thresholds {
    fn default() -> Self {
        Self {
            t_stuck: 0.85,
            t_progress: 0.30,
            t_low: 0.30,
            t_high: 0.80,
        }
    }
}
/// Model and purpose.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CalibrationKey {
    /// Dated model version.
    pub model_version: String,
    /// Decision purpose.
    pub purpose: String,
}
/// Calibration persistence interface.
pub trait CalibrationStore {
    /// Looks up thresholds.
    fn get(&self, key: &CalibrationKey) -> Option<Thresholds>;
    /// Stores thresholds.
    fn put(&self, key: CalibrationKey, t: Thresholds);
}
/// Volatile calibration store.
#[derive(Default)]
pub struct InMemoryCalibration {
    values: Mutex<HashMap<CalibrationKey, Thresholds>>,
}
impl CalibrationStore for InMemoryCalibration {
    fn get(&self, key: &CalibrationKey) -> Option<Thresholds> {
        self.values
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(key)
            .copied()
    }
    fn put(&self, key: CalibrationKey, t: Thresholds) {
        self.values
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(key, t);
    }
}
/// Decision authorization.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Action {
    /// Calibrated thresholds.
    Act(Thresholds),
    /// Record only.
    LogOnly,
}
/// Returns the action allowed for this key.
#[must_use]
pub fn action_for(store: &impl CalibrationStore, key: &CalibrationKey) -> Action {
    store.get(key).map_or(Action::LogOnly, Action::Act)
}
/// Fit policy.
#[derive(Debug, Clone, Copy)]
pub struct FitConfig {
    /// Minimum recall.
    pub min_recall: f64,
    /// Desired precision.
    pub precision_target: f64,
}
impl Default for FitConfig {
    fn default() -> Self {
        Self {
            min_recall: 0.5,
            precision_target: 0.9,
        }
    }
}
/// Fitted threshold and scores.
#[derive(Debug, Clone, Copy)]
pub struct FitResult {
    /// Cut point.
    pub threshold: f64,
    /// Precision.
    pub precision: f64,
    /// Recall.
    pub recall: f64,
}
impl FitResult {
    /// Checks the desired precision.
    #[must_use]
    pub fn meets_target(&self, cfg: &FitConfig) -> bool {
        self.precision >= cfg.precision_target
    }
}
/// Fits an upper cut point.
#[must_use]
pub fn fit_upper(samples: &[(f64, bool)], cfg: &FitConfig) -> Option<FitResult> {
    fit(samples, cfg, true)
}
/// Fits a lower cut point.
#[must_use]
pub fn fit_lower(samples: &[(f64, bool)], cfg: &FitConfig) -> Option<FitResult> {
    fit(samples, cfg, false)
}
#[allow(clippy::cast_precision_loss)] // Sample counts are bounded by addressable memory.
fn fit(samples: &[(f64, bool)], cfg: &FitConfig, upper: bool) -> Option<FitResult> {
    let positives = samples
        .iter()
        .filter(|(p, label)| p.is_finite() && *label)
        .count();
    if positives == 0 || !cfg.min_recall.is_finite() {
        return None;
    }
    let mut best: Option<FitResult> = None;
    for &(threshold, _) in samples.iter().filter(|(p, _)| p.is_finite()) {
        let mut predicted = 0usize;
        let mut true_positive = 0usize;
        for &(probability, label) in samples {
            if probability.is_finite()
                && if upper {
                    probability >= threshold
                } else {
                    probability <= threshold
                }
            {
                predicted += 1;
                true_positive += usize::from(label);
            }
        }
        let precision = true_positive as f64 / predicted as f64;
        let recall = true_positive as f64 / positives as f64;
        if recall >= cfg.min_recall
            && best.is_none_or(|b| {
                precision > b.precision
                    || (precision.total_cmp(&b.precision).is_eq() && recall > b.recall)
            })
        {
            best = Some(FitResult {
                threshold,
                precision,
                recall,
            });
        }
    }
    best
}
/// Checks agreement of two probabilities.
#[must_use]
pub fn agree(a: Option<f64>, b: Option<f64>, threshold: f64) -> bool {
    a.zip(b)
        .is_some_and(|(a, b)| a >= threshold && b >= threshold)
}
/// Shadow-mode observation window.
pub struct ShadowWindow {
    /// Start time in Unix milliseconds.
    pub started_ms: i64,
    /// Number of judged turns.
    pub judged_turns: u64,
}
impl ShadowWindow {
    /// True until both requirements are satisfied.
    #[must_use]
    pub fn is_shadow(&self, now_ms: i64) -> bool {
        now_ms.saturating_sub(self.started_ms) < 14 * 86_400_000 || self.judged_turns < 200
    }
}
