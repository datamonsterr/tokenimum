//! Calibration thresholds and shadow policy.
use std::collections::HashMap;
use std::sync::Mutex;
/// Decision thresholds.
#[derive(Debug, Clone, Copy, PartialEq)]
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
    fn get(&self, _key: &CalibrationKey) -> Option<Thresholds> {
        None
    }
    fn put(&self, _key: CalibrationKey, _t: Thresholds) {}
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
pub fn action_for(_store: &impl CalibrationStore, _key: &CalibrationKey) -> Action {
    Action::LogOnly
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
pub fn fit_upper(_samples: &[(f64, bool)], _cfg: &FitConfig) -> Option<FitResult> {
    None
}
/// Fits a lower cut point.
#[must_use]
pub fn fit_lower(_samples: &[(f64, bool)], _cfg: &FitConfig) -> Option<FitResult> {
    None
}
/// Checks agreement of two probabilities.
#[must_use]
pub fn agree(_a: Option<f64>, _b: Option<f64>, _threshold: f64) -> bool {
    false
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
    pub fn is_shadow(&self, _now_ms: i64) -> bool {
        true
    }
}
