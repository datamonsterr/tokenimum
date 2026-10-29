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
/// critical value. For df below 100, numerical inversion of the regularized
/// incomplete beta function is used; at df 100 and above a two-term asymptotic
/// expansion is used. Regression checks require relative error below 0.5%, with
/// tighter checks at the independently tabulated integer degrees of freedom.
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

// Invert the t distribution using its regularized incomplete beta tail;
// DLMF 8.17.4 and 8.17.22: https://dlmf.nist.gov/8.17
// Bisection on [0, 16] contains the 97.5th percentile for every df >= 1.
fn t_critical(df: f64) -> f64 {
    if df >= 100.0 {
        let z = 1.959_963_984_540_054_f64;
        return z
            + (z.powi(3) + z) / (4.0 * df)
            + (5.0 * z.powi(5) + 16.0 * z.powi(3) + 3.0 * z) / (96.0 * df.powi(2));
    }
    let (mut lo, mut hi) = (0.0, 16.0);
    for _ in 0..60 {
        let mid = f64::midpoint(lo, hi);
        let x = df / (df + mid * mid);
        if regularized_beta(x, df / 2.0, 0.5) > 0.05 {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    f64::midpoint(lo, hi)
}

// Lanczos approximation, g=7; valid for the positive arguments used here.
#[allow(clippy::cast_precision_loss)]
fn log_gamma(z: f64) -> f64 {
    const C: [f64; 9] = [
        0.999_999_999_999_809_9,
        676.520_368_121_885_1,
        -1_259.139_216_722_402_8,
        771.323_428_777_653_1,
        -176.615_029_162_140_6,
        12.507_343_278_686_905,
        -0.138_571_095_265_720_12,
        9.984_369_578_019_572e-6,
        1.505_632_735_149_311_6e-7,
    ];
    let v = z - 1.0;
    let mut sum = C[0];
    for (i, coefficient) in C.iter().enumerate().skip(1) {
        sum += coefficient / (v + i as f64);
    }
    let t = v + 7.5;
    0.918_938_533_204_672_7 + (v + 0.5) * t.ln() - t + sum.ln()
}

// Modified Lentz evaluation of the DLMF continued fraction, with symmetry to
// select the rapidly convergent side. The domain here has a,b >= 0.5.
#[allow(clippy::many_single_char_names)] // Standard continued-fraction notation.
fn beta_fraction(a: f64, b: f64, x: f64) -> f64 {
    const TINY: f64 = 1e-300;
    let qab = a + b;
    let qap = a + 1.0;
    let qam = a - 1.0;
    let mut c = 1.0;
    let mut d = 1.0 - qab * x / qap;
    if d.abs() < TINY {
        d = TINY;
    }
    d = 1.0 / d;
    let mut h = d;
    for m in 1..=400_u32 {
        let m = f64::from(m);
        let m2 = 2.0 * m;
        let aa = m * (b - m) * x / ((qam + m2) * (a + m2));
        d = 1.0 + aa * d;
        if d.abs() < TINY {
            d = TINY;
        }
        c = 1.0 + aa / c;
        if c.abs() < TINY {
            c = TINY;
        }
        d = 1.0 / d;
        h *= d * c;
        let aa = -(a + m) * (qab + m) * x / ((a + m2) * (qap + m2));
        d = 1.0 + aa * d;
        if d.abs() < TINY {
            d = TINY;
        }
        c = 1.0 + aa / c;
        if c.abs() < TINY {
            c = TINY;
        }
        d = 1.0 / d;
        let delta = d * c;
        h *= delta;
        if (delta - 1.0).abs() < 1e-14 {
            break;
        }
    }
    h
}

fn regularized_beta(x: f64, a: f64, b: f64) -> f64 {
    let factor =
        (log_gamma(a + b) - log_gamma(a) - log_gamma(b) + a * x.ln() + b * (-x).ln_1p()).exp();
    if x < (a + 1.0) / (a + b + 2.0) {
        factor * beta_fraction(a, b, x) / a
    } else {
        1.0 - factor * beta_fraction(b, a, 1.0 - x) / b
    }
}

#[cfg(test)]
mod tests {
    use super::t_critical;

    #[test]
    fn student_t_quantiles_for_fractional_and_integer_degrees_of_freedom() {
        for (df, expected) in [
            (1.0, 12.706_205),
            (1.5, 6.016_663),
            (2.0, 4.302_653),
            (10.0, 2.228_139),
            (30.0, 2.042_272),
            (1_000_000.0, 1.959_966),
        ] {
            let actual = t_critical(df);
            assert!(
                (actual - expected).abs() / expected < 0.000_001,
                "df={df}, got {actual}, expected {expected}"
            );
        }
    }
}
