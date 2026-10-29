//! Decision threshold calibration command entry point.
use crate::{
    cli::{CalibrateArgs, CalibrationPurpose},
    ctx::Ctx,
    daemon,
    error::CliError,
};
use std::{collections::BTreeMap, process::ExitCode};
use tkm_decide::calibrate::{FitConfig, Thresholds, fit_upper};
const MIN_LABELS: usize = 20;

/// Fit thresholds from labelled decisions.
///
/// # Errors
/// Returns an error for database or output failures.
pub fn run(ctx: &mut Ctx<'_>, args: &CalibrateArgs) -> Result<ExitCode, CliError> {
    if args
        .purpose
        .is_none_or(|purpose| purpose == CalibrationPurpose::Loop)
    {
        fit_loop(ctx)?;
    }
    if args
        .purpose
        .is_none_or(|purpose| purpose == CalibrationPurpose::Facts)
    {
        writeln!(
            ctx.stdout,
            "facts: calibration not available yet (no label contract)"
        )?;
    }
    if args
        .purpose
        .is_none_or(|purpose| purpose == CalibrationPurpose::Relevance)
    {
        writeln!(
            ctx.stdout,
            "relevance: calibration not available yet (no label contract)"
        )?;
    }
    Ok(ExitCode::SUCCESS)
}
fn fit_loop(ctx: &mut Ctx<'_>) -> Result<(), CliError> {
    let db = daemon::open_db(&ctx.paths)?;
    let mut stmt = db.prepare("SELECT model, answers, outcome_label FROM decisions WHERE purpose = 'loop_guard' AND outcome_label IN ('stuck', 'progress') ORDER BY id").map_err(db_error)?;
    let rows = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })
        .map_err(db_error)?;
    let mut groups: BTreeMap<String, Vec<(f64, bool)>> = BTreeMap::new();
    for row in rows {
        let (model, answers, label) = row.map_err(db_error)?;
        let answers: serde_json::Value = serde_json::from_str(&answers)
            .map_err(|e| CliError::Usage(format!("invalid decision answers: {e}")))?;
        if let Some(value) = answers
            .get("stuck")
            .and_then(|v| v.get("noul"))
            .and_then(serde_json::Value::as_f64)
            .filter(|v| (0.0..=1.0).contains(v))
        {
            groups
                .entry(model)
                .or_default()
                .push((value, label == "stuck"));
        }
    }
    if groups.is_empty() {
        writeln!(ctx.stdout, "loop: too few labels (0; need {MIN_LABELS})")?;
    }
    let policy = FitConfig::default();
    let mut fitted = Vec::new();
    for (model, samples) in groups {
        if samples.len() < MIN_LABELS {
            writeln!(
                ctx.stdout,
                "{model}: too few labels ({}; need {MIN_LABELS})",
                samples.len()
            )?;
            continue;
        }
        let Some(fit) = fit_upper(&samples, &policy).filter(|fit| fit.meets_target(&policy)) else {
            writeln!(
                ctx.stdout,
                "{model}: no threshold meets precision and recall targets ({} labels)",
                samples.len()
            )?;
            continue;
        };
        let defaults = Thresholds::default();
        if fit.threshold <= defaults.t_low {
            writeln!(
                ctx.stdout,
                "{model}: refusing fit: t_stuck must be greater than t_low ({})",
                defaults.t_low
            )?;
            continue;
        }
        let count = u32::try_from(samples.len())
            .map_err(|_| CliError::Usage("too many calibration samples".into()))?;
        writeln!(
            ctx.stdout,
            "{model}: proposed t_stuck = {} ({} samples, precision {}, recall {})",
            fit.threshold, count, fit.precision, fit.recall
        )?;
        fitted.push(tkm_store::CalibrationRecord { model_version: model, purpose: "loop_guard".into(), thresholds: serde_json::json!({"t_stuck": fit.threshold, "t_progress": defaults.t_progress, "t_low": defaults.t_low, "t_high": defaults.t_high}), sample_count: count, precision: fit.precision, recall: fit.recall });
    }
    if !fitted.is_empty() {
        let store = daemon::open_db_for_write(&ctx.paths)?;
        for result in &fitted {
            store
                .upsert_calibration(result)
                .map_err(|e| CliError::Usage(format!("cannot save calibration: {e}")))?;
        }
    }
    Ok(())
}
fn db_error(error: rusqlite::Error) -> CliError {
    CliError::Usage(format!("database query failed: {error}"))
}
