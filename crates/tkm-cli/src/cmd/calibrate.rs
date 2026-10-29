//! Decision threshold calibration, isolated by model version and purpose.
use crate::{
    cli::{CalibrateArgs, CalibrationPurpose},
    ctx::Ctx,
    daemon,
    error::CliError,
};
use std::{collections::BTreeMap, process::ExitCode};
use tkm_decide::calibrate::{
    LabelledSample, MIN_LABELS, Thresholds, fit_purpose, labelled_samples,
};
mod fixtures;
type Groups = BTreeMap<(String, String), Vec<LabelledSample>>;

/// Fit and persist thresholds from labelled decisions or offline fixture outcomes.
///
/// # Errors
/// Returns an error for invalid fixture input, database, or output failures.
#[allow(clippy::too_many_lines)] // Group, report and persist one calibration command atomically.
pub fn run(ctx: &mut Ctx<'_>, args: &CalibrateArgs) -> Result<ExitCode, CliError> {
    let mut groups = Groups::new();
    if let Some(path) = &args.fixtures {
        if selected(args, "loop_guard") {
            for (model, samples) in super::bench::load(path)?.samples {
                groups.insert(
                    (model, "loop_guard".into()),
                    samples
                        .into_iter()
                        .map(|(primary, positive)| LabelledSample {
                            primary,
                            secondary: None,
                            positive,
                        })
                        .collect(),
                );
            }
        }
        fixtures::load(path, args, &mut groups)?;
    } else {
        let db = daemon::open_db(&ctx.paths)?;
        let mut stmt = db.prepare("SELECT model, purpose, answers, outcome_label FROM decisions WHERE outcome_label IS NOT NULL ORDER BY id").map_err(db_error)?;
        let rows = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                ))
            })
            .map_err(db_error)?;
        for row in rows {
            let (model, purpose, answers, label) = row.map_err(db_error)?;
            if !selected(args, &purpose) {
                continue;
            }
            let answers: serde_json::Value = serde_json::from_str(&answers)
                .map_err(|e| CliError::Usage(format!("invalid decision answers: {e}")))?;
            let labels =
                serde_json::from_str(&label).unwrap_or_else(|_| serde_json::Value::String(label));
            groups
                .entry((model, purpose.clone()))
                .or_default()
                .extend(labelled_samples(&purpose, &answers, &labels));
        }
    }
    for purpose in [
        "loop_guard",
        "fact_filter",
        "fact_contradiction",
        "relevance",
    ] {
        if selected(args, purpose) && !groups.keys().any(|(_, p)| p == purpose) {
            writeln!(
                ctx.stdout,
                "{}: too few labels (0; need {MIN_LABELS})",
                display(purpose)
            )?;
        }
    }
    let mut fitted = Vec::new();
    for ((model, purpose), samples) in groups {
        if samples.len() < MIN_LABELS {
            writeln!(
                ctx.stdout,
                "{model} {purpose}: too few labels ({}; need {MIN_LABELS})",
                samples.len()
            )?;
            continue;
        }
        let Some(fit) = fit_purpose(&purpose, &samples) else {
            writeln!(
                ctx.stdout,
                "{model} {purpose}: no threshold meets precision and recall targets ({} labels)",
                samples.len()
            )?;
            continue;
        };
        if purpose == "loop_guard" && fit.thresholds.t_stuck <= Thresholds::default().t_low {
            writeln!(
                ctx.stdout,
                "{model}: refusing fit: t_stuck must be greater than t_low ({})",
                Thresholds::default().t_low
            )?;
            continue;
        }
        let t = fit.thresholds;
        let count = u32::try_from(fit.sample_count)
            .map_err(|_| CliError::Usage("too many calibration samples".into()))?;
        writeln!(
            ctx.stdout,
            "{model} {purpose}: proposed thresholds ({} samples, precision {}, recall {})",
            count, fit.precision, fit.recall
        )?;
        fitted.push(tkm_store::CalibrationRecord { model_version:model, purpose,
            thresholds:serde_json::json!({"t_stuck":t.t_stuck,"t_progress":t.t_progress,"t_low":t.t_low,"t_high":t.t_high}), sample_count:count, precision:fit.precision, recall:fit.recall });
    }
    if !fitted.is_empty() {
        let store = daemon::open_db_for_write(&ctx.paths)?;
        for row in &fitted {
            store
                .upsert_calibration(row)
                .map_err(|e| CliError::Usage(format!("cannot save calibration: {e}")))?;
        }
    }
    Ok(ExitCode::SUCCESS)
}
fn selected(args: &CalibrateArgs, purpose: &str) -> bool {
    match args.purpose {
        None => true,
        Some(CalibrationPurpose::Loop) => purpose == "loop_guard",
        Some(CalibrationPurpose::Facts) => matches!(purpose, "fact_filter" | "fact_contradiction"),
        Some(CalibrationPurpose::Relevance) => purpose == "relevance",
    }
}
fn display(purpose: &str) -> &str {
    if purpose == "loop_guard" {
        "loop"
    } else {
        purpose
    }
}
fn db_error(error: rusqlite::Error) -> CliError {
    CliError::Usage(format!("database query failed: {error}"))
}
