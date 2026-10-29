use super::{Baseline, BenchError, SessionMetrics, Tokens, types::invalid};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::io::Write;

/// CI-equivalent outcome checks, separate from token usage.
#[derive(Debug, Default, Clone, Copy, Serialize, Deserialize)]
pub struct Outcome {
    /// All project test commands completed successfully, with a successful Claude result.
    pub tests_passed: bool,
    /// Jaccard overlap of changed file sets, from zero to one (both empty is one).
    pub changed_file_overlap: f64,
}
impl Outcome {
    fn passed(self) -> bool {
        self.tests_passed && (0.5..=1.0).contains(&self.changed_file_overlap)
    }
}

/// Cost comparison after the control-arm noise floor and outcome checks.
#[derive(Debug, Serialize)]
pub struct Comparison {
    /// Relative reduction in priced cost, including decision overhead.
    pub reduction: f64,
    /// Maximum absolute relative baseline/control deviation across controls.
    pub variance: f64,
    /// Outcome passed and reduction strictly exceeds the noise floor.
    pub saving: bool,
    /// Replay outcome failed, irrespective of its token count.
    pub regression: bool,
}

/// Compare a session against a conservative empirical control noise floor.
///
/// # Errors
/// Requires at least ten independent controls and finite, valid costs and outcomes.
pub fn compare(
    baseline: f64,
    replay: f64,
    controls: &[f64],
    outcome: Outcome,
) -> Result<Comparison, BenchError> {
    if controls.len() < 10 {
        return Err(invalid("at least ten control sessions are required"));
    }
    if !baseline.is_finite()
        || baseline <= 0.0
        || !replay.is_finite()
        || replay < 0.0
        || controls.iter().any(|v| !v.is_finite() || *v < 0.0)
        || !outcome.changed_file_overlap.is_finite()
        || !(0.0..=1.0).contains(&outcome.changed_file_overlap)
    {
        return Err(invalid("invalid comparison measurement"));
    }
    let variance = controls.iter().copied().fold(0.0_f64, f64::max);
    let reduction = (baseline - replay) / baseline;
    let regression = !outcome.passed();
    Ok(Comparison {
        reduction,
        variance,
        saving: !regression && reduction > variance,
        regression,
    })
}

/// One measured replay, with tokenimum overhead accounted for separately.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct Measurement {
    /// Claude transcript aggregates, including spawned agents.
    pub metrics: SessionMetrics,
    /// Replay outcome.
    pub outcome: Outcome,
    /// Decision-model tokens from tokenimum's exported ledgers.
    pub decision_tokens: u64,
    /// Decision-model USD cost from exported decision records.
    pub decision_cost_usd: f64,
    /// Exported decision count.
    pub decisions: u64,
    /// Exported redirect events.
    pub redirects: u64,
    /// Exported fold events.
    pub folds: u64,
}

/// Private per-session measurements. This JSON is never an uploaded report.
#[derive(Debug, Serialize, Deserialize)]
pub struct Measurements {
    /// Whether code was reset before each replay or continued across sessions.
    pub mode: String,
    /// Tokenimum arm, keyed by baseline session ID.
    pub replay: BTreeMap<String, Measurement>,
    /// No-tokenimum controls, keyed by distinct baseline session IDs.
    pub control: BTreeMap<String, Measurement>,
}

#[derive(Default)]
struct Totals {
    baseline: Tokens,
    replay: Tokens,
    original_cost: f64,
    replay_cost: f64,
    count: u64,
    savings: u64,
    regressions: u64,
}
impl Totals {
    fn add(&mut self, original: &SessionMetrics, replay: &Measurement, result: &Comparison) {
        self.baseline.add(original.tokens);
        self.replay.add(replay.metrics.tokens);
        self.original_cost += original.cost_usd;
        self.replay_cost += replay.metrics.cost_usd + replay.decision_cost_usd;
        self.count += 1;
        self.savings += u64::from(result.saving);
        self.regressions += u64::from(result.regression);
    }
}

/// Write aggregates only: totals, per-model classes and session-size buckets, without IDs/prompts.
///
/// # Errors
/// Refuses unmatched arms, missing baseline sessions, failed controls and insufficient controls.
#[allow(clippy::too_many_lines)] // Report tables are kept together to audit the aggregate-only boundary.
pub fn write_report(
    baseline: &Baseline,
    measurements: &Measurements,
    mut output: impl Write,
) -> Result<(), BenchError> {
    if measurements.replay.is_empty() {
        return Err(invalid("replay arm is empty"));
    }
    if measurements.mode != "anchored" && measurements.mode != "free-running" {
        return Err(invalid("invalid replay mode"));
    }
    let controls = measurements
        .control
        .iter()
        .map(|(id, measurement)| {
            let original = baseline
                .sessions
                .get(id)
                .ok_or_else(|| invalid("control has no baseline"))?;
            if !measurement.outcome.passed()
                || !original.cost_usd.is_finite()
                || original.cost_usd <= 0.0
                || !measurement.metrics.cost_usd.is_finite()
                || measurement.metrics.cost_usd < 0.0
                || measurement.decision_tokens != 0
                || measurement.decision_cost_usd != 0.0
                || measurement.decisions != 0
                || measurement.redirects != 0
                || measurement.folds != 0
            {
                return Err(invalid("control outcome failed or has zero baseline cost"));
            }
            Ok(((original.cost_usd - measurement.metrics.cost_usd) / original.cost_usd).abs())
        })
        .collect::<Result<Vec<_>, BenchError>>()?;
    let mut total = Totals::default();
    let mut buckets: BTreeMap<&str, Totals> = BTreeMap::new();
    let mut baseline_models: BTreeMap<&str, Tokens> = BTreeMap::new();
    let mut replay_models: BTreeMap<&str, Tokens> = BTreeMap::new();
    let mut overhead = (0_u64, 0.0_f64, 0_u64, 0_u64, 0_u64);
    let mut baseline_activity = [0_u64; 4];
    let mut replay_activity = [0_u64; 4];
    for (id, measurement) in &measurements.replay {
        let original = baseline
            .sessions
            .get(id)
            .ok_or_else(|| invalid("replay has no baseline"))?;
        if !measurement.decision_cost_usd.is_finite() || measurement.decision_cost_usd < 0.0 {
            return Err(invalid("invalid decision cost"));
        }
        let result = compare(
            original.cost_usd,
            measurement.metrics.cost_usd + measurement.decision_cost_usd,
            &controls,
            measurement.outcome,
        )?;
        total.add(original, measurement, &result);
        for (sums, metrics) in [
            (&mut baseline_activity, original),
            (&mut replay_activity, &measurement.metrics),
        ] {
            for (sum, value) in sums.iter_mut().zip([
                metrics.turns,
                metrics.tool_calls,
                metrics.tool_errors,
                metrics.wall_ms,
            ]) {
                *sum = sum.saturating_add(value);
            }
        }
        let bucket = match original.tokens.size() {
            0..=99_999 => "small (<100k)",
            100_000..=999_999 => "medium (100k–1M)",
            _ => "large (>=1M)",
        };
        buckets
            .entry(bucket)
            .or_default()
            .add(original, measurement, &result);
        for (model, tokens) in &original.by_model {
            baseline_models.entry(model).or_default().add(*tokens);
        }
        for (model, tokens) in &measurement.metrics.by_model {
            replay_models.entry(model).or_default().add(*tokens);
        }
        overhead.0 += measurement.decision_tokens;
        overhead.1 += measurement.decision_cost_usd;
        overhead.2 += measurement.decisions;
        overhead.3 += measurement.redirects;
        overhead.4 += measurement.folds;
    }
    writeln!(
        output,
        "# Replay benchmark\n\nMode: {}. Sessions: {}. Skipped transcripts: {}. Controls: {}.",
        measurements.mode,
        total.count,
        baseline.skipped.len(),
        controls.len()
    )?;
    let aggregate_outcome = Outcome {
        tests_passed: total.regressions == 0,
        changed_file_overlap: if total.regressions == 0 { 1.0 } else { 0.0 },
    };
    let overall = compare(
        total.original_cost,
        total.replay_cost,
        &controls,
        aggregate_outcome,
    )?;
    writeln!(
        output,
        "\nControl variance rule: maximum absolute relative cost deviation = {:.2}%. A saving requires a strictly larger reduction and passing tests plus changed-file Jaccard overlap >=0.5. Failed controls invalidate the comparison.",
        overall.variance * 100.0
    )?;
    writeln!(
        output,
        "\n| Arm | Input | Cache creation | Cache read | Output | Cost USD |\n|---|---:|---:|---:|---:|---:|"
    )?;
    token_row(
        &mut output,
        "Recorded baseline (matched subset)",
        total.baseline,
        total.original_cost,
    )?;
    token_row(
        &mut output,
        "Tokenimum replay (cost includes decisions)",
        total.replay,
        total.replay_cost,
    )?;
    writeln!(
        output,
        "\nCost reduction: {:.2}%. Qualifying total saving: {}. Outcome regressions: {}. Sessions with qualifying savings: {}.\n\nDecision overhead: {} tokens, ${:.6}; decisions: {}, redirects: {}, folds: {}.",
        overall.reduction * 100.0,
        overall.saving,
        total.regressions,
        total.savings,
        overhead.0,
        overhead.1,
        overhead.2,
        overhead.3,
        overhead.4
    )?;
    writeln!(
        output,
        "\n| Arm activity | Turns | Tool calls | Tool errors | Sum of session wall ms |\n|---|---:|---:|---:|---:|"
    )?;
    for (label, activity) in [("baseline", baseline_activity), ("replay", replay_activity)] {
        writeln!(
            output,
            "| {label} | {} | {} | {} | {} |",
            activity[0], activity[1], activity[2], activity[3]
        )?;
    }
    writeln!(
        output,
        "\n| Model / arm | Input | Cache creation | Cache read | Output |\n|---|---:|---:|---:|---:|"
    )?;
    for (arm, models) in [("baseline", baseline_models), ("replay", replay_models)] {
        for (model, tokens) in models {
            if !model
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"-._/".contains(&b))
            {
                return Err(invalid("unsafe model label"));
            }
            writeln!(
                output,
                "| {model} / {arm} | {} | {} | {} | {} |",
                tokens.input, tokens.cache_creation, tokens.cache_read, tokens.output
            )?;
        }
    }
    writeln!(
        output,
        "\n| Baseline session size | Sessions | Baseline USD | Replay + decisions USD | Savings above variance | Regressions |\n|---|---:|---:|---:|---:|---:|"
    )?;
    for (bucket, value) in buckets {
        writeln!(
            output,
            "| {bucket} | {} | {:.6} | {:.6} | {} | {} |",
            value.count, value.original_cost, value.replay_cost, value.savings, value.regressions
        )?;
    }
    writeln!(
        output,
        "\nSkipped sessions remain listed by ID and reason only in private baseline.json. This artifact contains no session IDs, paths, prompts, diffs or model output. Historical prices are supplied by the bundle; these are estimates, not invoices."
    )?;
    Ok(())
}
fn token_row(
    output: &mut impl Write,
    label: &str,
    tokens: Tokens,
    cost: f64,
) -> Result<(), BenchError> {
    writeln!(
        output,
        "| {label} | {} | {} | {} | {} | {cost:.6} |",
        tokens.input, tokens.cache_creation, tokens.cache_read, tokens.output
    )?;
    Ok(())
}
