//! Decision model diagnostics command entry point.
use crate::{
    cli::{ModelsArgs, ModelsCommand},
    ctx::Ctx,
    daemon,
    error::CliError,
};
use std::{collections::BTreeMap, process::ExitCode, sync::Arc};
use tkm_decide::{
    api::Question,
    budget::InMemoryLedger,
    cache::Purpose,
    chain::SystemClock,
    client::{DecideRequest, DecisionClient, HttpTransport, Outcome},
    key::KeyError,
};

/// List configured models or probe their availability.
///
/// # Errors
/// Returns an error for database, key, runtime, transport, or output failures.
pub fn run(ctx: &mut Ctx<'_>, args: &ModelsArgs) -> Result<ExitCode, CliError> {
    let config = ctx.load_config()?;
    let settings = tkmd::settings::Settings::from_config(&config);
    let db = daemon::open_db(&ctx.paths)?;
    let mut statement = db.prepare("SELECT purpose, sample_count FROM calibration WHERE model_version = ?1 ORDER BY purpose").map_err(db_error)?;
    for model in &settings.decide.chain {
        let rows = statement
            .query_map([&model.slug], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, u32>(1)?))
            })
            .map_err(db_error)?;
        let calibrations = rows
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(db_error)?;
        write!(ctx.stdout, "{}: availability unknown", model.slug)?;
        if calibrations.is_empty() {
            writeln!(ctx.stdout, "; uncalibrated")?;
        } else {
            for (purpose, count) in calibrations {
                write!(ctx.stdout, "; {purpose}: {count} samples")?;
            }
            writeln!(ctx.stdout)?;
        }
    }
    if let Some(ModelsCommand::Probe(_)) = &args.command {
        probe(ctx, &config, settings.decide)?;
    }
    Ok(ExitCode::SUCCESS)
}
fn db_error(error: rusqlite::Error) -> CliError {
    CliError::Usage(format!("database query failed: {error}"))
}
#[allow(clippy::too_many_lines)] // Probe execution and accounting stay in one bounded workflow.
fn probe(
    ctx: &mut Ctx<'_>,
    effective: &tkm_config::config::EffectiveConfig,
    config: tkm_decide::config::DecideConfig,
) -> Result<(), CliError> {
    let env = ctx.env.clone();
    let var = |name: &str| {
        env.iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.clone())
    };
    let key = match super::init::key::load(ctx, effective) {
        Ok(key) if config.enabled => key,
        Ok(_) | Err(KeyError::Missing) => {
            writeln!(ctx.stdout, "skipped (offline)")?;
            return Ok(());
        }
        Err(error) => {
            return Err(CliError::Usage(format!(
                "cannot load decision key: {error}"
            )));
        }
    };
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    let store = daemon::open_db_for_write(&ctx.paths)?;
    let ledger = Arc::new(InMemoryLedger::default());
    let mut recorded = 0;
    for model in &config.chain {
        let mut one = config.clone();
        one.chain = vec![model.clone()];
        let transport = match var("TKM_DECIDE_ENDPOINT") {
            Some(url) => HttpTransport::with_endpoint(&url),
            None => HttpTransport::new(),
        }
        .map_err(|e| CliError::Usage(e.to_string()))?;
        let client = DecisionClient::new(
            one,
            Some(key.clone()),
            transport,
            SystemClock,
            Arc::clone(&ledger),
        );
        let request = DecideRequest {
            purpose: Purpose::LoopGuard,
            session_id: "tkm-model-probe".into(),
            state: "A diagnostic request.".into(),
            questions: BTreeMap::from([(
                "probe".into(),
                Question::Noul {
                    instructions: "Is this a diagnostic?".into(),
                    criteria: None,
                },
            )]),
        };
        let questions =
            serde_json::to_value(&request.questions).map_err(super::status::report::json_error)?;
        let outcomes = runtime.block_on(async {
            if model.free_probe || model.slug.ends_with(":free") {
                client.reprobe_free(request).await
            } else {
                vec![client.decide(request).await]
            }
        });
        let available = outcomes
            .iter()
            .any(|outcome| matches!(outcome, Outcome::Decision(_)));
        let answers = outcomes
            .iter()
            .find_map(|outcome| match outcome {
                Outcome::Decision(decision) => Some(&decision.answers),
                _ => None,
            })
            .map(serde_json::to_value)
            .transpose()
            .map_err(super::status::report::json_error)?
            .unwrap_or_else(|| serde_json::json!({}));
        let attempts = ledger.rows();
        for attempt in attempts.iter().skip(recorded) {
            store
                .insert_decision_with_context(
                    &tkm_store::NewDecision {
                        model: attempt
                            .model_version
                            .as_ref()
                            .unwrap_or(&attempt.requested_model)
                            .clone(),
                        purpose: "models_probe".into(),
                        questions: questions.clone(),
                        answers: answers.clone(),
                        cost_usd: attempt.cost_usd,
                        latency_ms: u32::try_from(attempt.latency_ms).unwrap_or(u32::MAX),
                        shadow: true,
                    },
                    Some(tkm_core::model::Timestamp(
                        i64::try_from(attempt.ts_ms).unwrap_or(i64::MAX),
                    )),
                    Some(&ctx.repo.key),
                    None,
                    None,
                )
                .map_err(super::init::state::failure)?;
        }
        recorded = attempts.len();
        writeln!(
            ctx.stdout,
            "{}: {}",
            model.slug,
            if available {
                "available"
            } else {
                "unavailable"
            }
        )?;
    }
    Ok(())
}
