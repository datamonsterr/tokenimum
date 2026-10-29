use super::valid_cost;
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
/// Welch t 95% interval for the difference of mean costs.
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
/// No first action means all sessions are baseline. Invalid or non-finite costs and
/// arithmetic return `None`. With fewer than two samples per arm the band is absent.
/// The 95% band uses the Welch-Satterthwaite degrees of freedom and a two-sided t
/// critical value. Table values for integer df 1–30 and 40, 60, 120 are rounded to
/// six decimals; linear interpolation between entries is approximate and conservative
/// for the convex t-quantile curve. Above 120 an asymptotic expansion is used.
#[must_use]
#[allow(clippy::cast_precision_loss)]
pub fn estimate_savings(
    sessions: &[SavingsSession],
    repo: &RepoKey,
    host: Host,
    first_action: Option<Timestamp>,
) -> Option<SavingsEstimate> {
    let (baseline, treatment): (Vec<_>, Vec<_>) = sessions
        .iter()
        .filter(|s| &s.repo == repo && s.host == host)
        .partition(|s| s.shadow || first_action.is_none_or(|first| s.started.0 < first.0));
    if baseline
        .iter()
        .chain(&treatment)
        .any(|s| !valid_cost(s.cost))
    {
        return None;
    }
    let mean = |values: &[&SavingsSession]| -> Option<Option<f64>> {
        if values.is_empty() {
            return Some(None);
        }
        let total: f64 = values.iter().map(|s| s.cost).sum();
        if !valid_cost(total) {
            return None;
        }
        Some(Some(total / values.len() as f64))
    };
    let bm = mean(&baseline)?;
    let tm = mean(&treatment)?;
    let difference = bm.zip(tm).map(|(b, t)| b - t);
    if difference.is_some_and(|d| !d.is_finite()) {
        return None;
    }
    let band = if baseline.len() >= 2 && treatment.len() >= 2 {
        let variance = |values: &[&SavingsSession], m: f64| -> Option<f64> {
            let sum: f64 = values.iter().map(|s| (s.cost - m).powi(2)).sum();
            valid_cost(sum).then_some(sum / (values.len() - 1) as f64)
        };
        let a = variance(&baseline, bm?)? / baseline.len() as f64;
        let b = variance(&treatment, tm?)? / treatment.len() as f64;
        let se2 = a + b;
        if !valid_cost(se2) {
            return None;
        }
        let margin = if se2 == 0.0 {
            0.0
        } else {
            // Normalize first to avoid overflow in the squared terms.
            let denom = (a / se2).powi(2) / (baseline.len() - 1) as f64
                + (b / se2).powi(2) / (treatment.len() - 1) as f64;
            t_critical(1.0 / denom) * se2.sqrt()
        };
        let d = difference?;
        let lower = d - margin;
        let upper = d + margin;
        if !lower.is_finite() || !upper.is_finite() {
            return None;
        }
        Some(ConfidenceBand { lower, upper })
    } else {
        None
    };
    Some(SavingsEstimate {
        baseline_count: baseline.len(),
        treatment_count: treatment.len(),
        baseline_mean: bm,
        treatment_mean: tm,
        difference,
        band,
    })
}

/// Validate costs and estimate savings; equivalent to [`estimate_savings`].
#[must_use]
pub fn checked_estimate_savings(
    sessions: &[SavingsSession],
    repo: &RepoKey,
    host: Host,
    first_action: Option<Timestamp>,
) -> Option<SavingsEstimate> {
    estimate_savings(sessions, repo, host, first_action)
}

#[allow(clippy::cast_precision_loss)]
fn t_critical(df: f64) -> f64 {
    // Two-sided 95% t table, e.g. https://www2.stat.duke.edu/courses/Spring11/sta114/exams/114s11fe.pdf
    const VALUES: [(f64, f64); 33] = [
        (1.0, 12.706205),
        (2.0, 4.302653),
        (3.0, 3.182446),
        (4.0, 2.776445),
        (5.0, 2.570582),
        (6.0, 2.446912),
        (7.0, 2.364624),
        (8.0, 2.306004),
        (9.0, 2.262157),
        (10.0, 2.228139),
        (11.0, 2.200985),
        (12.0, 2.178813),
        (13.0, 2.160369),
        (14.0, 2.144787),
        (15.0, 2.131450),
        (16.0, 2.119905),
        (17.0, 2.109816),
        (18.0, 2.100922),
        (19.0, 2.093024),
        (20.0, 2.085963),
        (21.0, 2.079614),
        (22.0, 2.073873),
        (23.0, 2.068658),
        (24.0, 2.063899),
        (25.0, 2.059539),
        (26.0, 2.055529),
        (27.0, 2.051831),
        (28.0, 2.048407),
        (29.0, 2.045230),
        (30.0, 2.042272),
        (40.0, 2.021075),
        (60.0, 2.000298),
        (120.0, 1.979930),
    ];
    for pair in VALUES.windows(2) {
        let (x0, y0) = pair[0];
        let (x1, y1) = pair[1];
        if df <= x1 {
            return y0 + (y1 - y0) * ((df - x0) / (x1 - x0)).clamp(0.0, 1.0);
        }
    }
    let z = 1.959_963_984_540_054_f64;
    z + (z.powi(3) + z) / (4.0 * df)
        + (5.0 * z.powi(5) + 16.0 * z.powi(3) + 3.0 * z) / (96.0 * df.powi(2))
}
