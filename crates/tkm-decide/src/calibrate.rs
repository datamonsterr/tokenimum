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

/// Minimum labelled items before a model/purpose fit can authorize decisions.
pub const MIN_LABELS: usize = 20;

/// Precision required to leave shadow for a supported purpose.
/// SPEC 8.5 requires 0.9 for stuck; the same conservative target applies to the other
/// purposes until a separate product target is specified.
#[must_use]
pub fn precision_target(purpose: &str) -> Option<f64> {
    matches!(
        purpose,
        "loop_guard" | "fact_filter" | "fact_contradiction" | "relevance"
    )
    .then_some(0.9)
}

/// One independently labelled candidate or loop judgment, in probability units.
#[derive(Debug, Clone, Copy)]
pub struct LabelledSample {
    /// Stuck, stability, contradiction, or relevance (score divided by three).
    pub primary: f64,
    /// Progress for loops or safety for fact filters; absent for single-question purposes.
    pub secondary: Option<f64>,
    /// Whether the action's condition held in later outcome evidence.
    pub positive: bool,
}

/// Threshold pair with measured quality on the labelled input.
#[derive(Debug, Clone, Copy)]
pub struct PurposeFit {
    /// Purpose-specific thresholds, preserving the common store representation.
    pub thresholds: Thresholds,
    /// Number of independently labelled items.
    pub sample_count: usize,
    /// Precision of the complete action predicate (including both questions).
    pub precision: f64,
    /// Recall of the complete action predicate.
    pub recall: f64,
}

/// Decode outcome labels without turning unlabelled questions into negative examples.
/// Legacy loop labels remain the strings `stuck` and `progress`; batch labels are JSON
/// objects keyed by the exact question name, with boolean observed outcomes.
#[must_use]
pub fn labelled_samples(
    purpose: &str,
    answers: &serde_json::Value,
    labels: &serde_json::Value,
) -> Vec<LabelledSample> {
    let probability = |key: &str| {
        answers
            .get(key)?
            .get("noul")?
            .as_f64()
            .filter(|v| (0.0..=1.0).contains(v))
    };
    let mut samples = Vec::new();
    if purpose == "loop_guard" {
        let positive = match labels.as_str() {
            Some("stuck") => Some(true),
            Some("progress") => Some(false),
            _ => None,
        };
        if let (Some(primary), Some(positive)) = (probability("stuck"), positive) {
            samples.push(LabelledSample {
                primary,
                secondary: probability("progress"),
                positive,
            });
        }
        return samples;
    }
    let Some(labels) = labels.as_object() else {
        return samples;
    };
    for (key, label) in labels {
        let Some(positive) = label.as_bool() else {
            continue;
        };
        let sample = match purpose {
            "fact_filter" => key.strip_prefix("stable_").and_then(|id| {
                let safe_key = format!("safe_{id}");
                // Both questions must have outcome evidence for this same candidate.
                (labels.get(&safe_key)?.as_bool()? == positive).then_some(LabelledSample {
                    primary: probability(key)?,
                    secondary: Some(probability(&safe_key)?),
                    positive,
                })
            }),
            "fact_contradiction" if key.starts_with("contradicts_") => {
                probability(key).map(|primary| LabelledSample {
                    primary,
                    secondary: None,
                    positive,
                })
            }
            "relevance" if key.starts_with("rel_") => answers
                .get(key)
                .and_then(|a| a.get("score"))
                .and_then(serde_json::Value::as_f64)
                .filter(|score| (0.0..=3.0).contains(score))
                .map(|score| LabelledSample {
                    primary: score / 3.0,
                    secondary: None,
                    positive,
                }),
            _ => None,
        };
        samples.extend(sample);
    }
    samples
}

/// Fit the full action predicate for one purpose, at minimum recall and precision.
/// Facts fit stability/safety jointly; loops fit stuck/progress jointly. Relevance is
/// normalized from its ordered 0..3 score and cannot lower SPEC 7.2's admission cutoffs.
#[must_use]
#[allow(clippy::cast_precision_loss)] // In-memory sample counts cannot exceed addressable memory.
#[allow(clippy::too_many_lines)] // Fit and score the complete two-question predicate together.
pub fn fit_purpose(purpose: &str, samples: &[LabelledSample]) -> Option<PurposeFit> {
    let target = precision_target(purpose)?;
    if samples.len() < MIN_LABELS
        || samples.iter().any(|s| {
            !(0.0..=1.0).contains(&s.primary)
                || s.secondary.is_some_and(|v| !(0.0..=1.0).contains(&v))
        })
    {
        return None;
    }
    let positives = samples.iter().filter(|s| s.positive).count();
    if positives == 0 {
        return None;
    }
    let defaults = Thresholds::default();
    let mut secondary: Vec<f64> = samples.iter().filter_map(|s| s.secondary).collect();
    secondary.push(if purpose == "loop_guard" {
        defaults.t_progress
    } else {
        defaults.t_high
    });
    secondary.sort_by(f64::total_cmp);
    secondary.dedup();
    let mut primary: Vec<f64> = samples.iter().map(|s| s.primary).collect();
    primary.sort_by(f64::total_cmp);
    primary.dedup();
    let mut best: Option<PurposeFit> = None;
    for low in primary {
        for &high in &secondary {
            if purpose == "fact_filter" && low > high {
                continue;
            }
            let mut thresholds = defaults;
            match purpose {
                "loop_guard" => {
                    thresholds.t_stuck = low;
                    thresholds.t_progress = high;
                }
                "fact_filter" => {
                    thresholds.t_low = low;
                    thresholds.t_high = high;
                }
                "fact_contradiction" => thresholds.t_high = low,
                "relevance" => {
                    thresholds.t_low = low.max(0.5);
                    thresholds.t_high = low.max(0.5);
                }
                _ => return None,
            }
            let predicts = |s: &&LabelledSample| match purpose {
                "loop_guard" => s.primary >= low && s.secondary.is_none_or(|p| p <= high),
                "fact_filter" => s.primary >= low && s.secondary.is_some_and(|p| p >= high),
                "relevance" => s.primary >= thresholds.t_high,
                _ => s.primary >= low,
            };
            let predicted = samples.iter().filter(predicts).count();
            if predicted == 0 {
                continue;
            }
            let hits = samples.iter().filter(|s| predicts(s) && s.positive).count();
            let precision = hits as f64 / predicted as f64;
            let recall = hits as f64 / positives as f64;
            if precision >= target
                && recall >= FitConfig::default().min_recall
                && best.is_none_or(|b| {
                    precision > b.precision
                        || (precision.total_cmp(&b.precision).is_eq() && recall > b.recall)
                })
            {
                best = Some(PurposeFit {
                    thresholds,
                    sample_count: samples.len(),
                    precision,
                    recall,
                });
            }
        }
    }
    best
}
