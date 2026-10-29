use crate::model::{Host, RepoKey, Timestamp};
use serde::{Deserialize, Serialize};
/// A priced session with its shadow flag (SPEC 7.5).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SavingsSession {
    /// Repository identity.
    pub repo: RepoKey,
    /// Harness identity.
    pub host: Host,
    /// Session start time.
    pub started: Timestamp,
    /// Whether this session was shadow-only.
    pub shadow: bool,
    /// Total session cost in US dollars, including decision overhead.
    pub cost: f64,
}
/// Normal approximation 95% interval for the difference of mean costs.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ConfidenceBand {
    /// Lower bound, in US dollars.
    pub lower: f64,
    /// Upper bound, in US dollars.
    pub upper: f64,
}
/// Per-repository and host comparison, with sample counts always present.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SavingsEstimate {
    /// Number of baseline sessions.
    pub baseline_count: usize,
    /// Number of treatment sessions.
    pub treatment_count: usize,
    /// Baseline mean, absent for an empty arm.
    pub baseline_mean: Option<f64>,
    /// Treatment mean, absent for an empty arm.
    pub treatment_mean: Option<f64>,
    /// Baseline minus treatment; absent for an empty arm.
    pub difference: Option<f64>,
    /// 95% confidence band, absent if either arm has fewer than two observations.
    pub band: Option<ConfidenceBand>,
}
/// Estimate savings for one repository and host. Shadow sessions and sessions started
/// before the first action in the repository are baseline; all others are treatment.
/// With no first action, all sessions are baseline. The band uses 1.96 times the
/// Welch standard error, with unbiased sample variances; fewer than two observations
/// in either arm is insufficient to calculate a band.
#[must_use]
#[allow(clippy::cast_precision_loss)]
pub fn estimate_savings(
    sessions: &[SavingsSession],
    repo: &RepoKey,
    host: Host,
    first_action: Option<Timestamp>,
) -> SavingsEstimate {
    let (baseline, treatment): (Vec<_>, Vec<_>) = sessions
        .iter()
        .filter(|s| &s.repo == repo && s.host == host)
        .partition(|s| s.shadow || first_action.is_none_or(|first| s.started.0 < first.0));
    let mean = |values: &[&SavingsSession]| {
        (!values.is_empty())
            .then(|| values.iter().map(|s| s.cost).sum::<f64>() / values.len() as f64)
    };
    let bm = mean(&baseline);
    let tm = mean(&treatment);
    let difference = bm.zip(tm).map(|(b, t)| b - t);
    let band = difference.and_then(|d| {
        if baseline.len() < 2 || treatment.len() < 2 {
            return None;
        }
        let variance = |values: &[&SavingsSession], m: f64| {
            values.iter().map(|s| (s.cost - m).powi(2)).sum::<f64>() / (values.len() - 1) as f64
        };
        let margin = 1.96
            * (variance(&baseline, bm?) / baseline.len() as f64
                + variance(&treatment, tm?) / treatment.len() as f64)
                .sqrt();
        Some(ConfidenceBand {
            lower: d - margin,
            upper: d + margin,
        })
    });
    SavingsEstimate {
        baseline_count: baseline.len(),
        treatment_count: treatment.len(),
        baseline_mean: bm,
        treatment_mean: tm,
        difference,
        band,
    }
}
