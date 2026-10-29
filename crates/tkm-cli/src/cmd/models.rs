//! Decision model diagnostics command entry point.
use crate::{
    cli::{ModelsArgs, ModelsCommand},
    ctx::Ctx,
    daemon,
    error::CliError,
};
use std::{collections::BTreeMap, path::PathBuf, process::ExitCode};
use tkm_decide::{
    api::Question,
    budget::InMemoryLedger,
    cache::Purpose,
    chain::SystemClock,
    client::{DecideRequest, DecisionClient, HttpTransport, Outcome},
    key::{self, KeyError},
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
        probe(ctx, settings.decide)?;
    }
    Ok(ExitCode::SUCCESS)
}
fn db_error(error: rusqlite::Error) -> CliError {
    CliError::Usage(format!("database query failed: {error}"))
}
fn probe(ctx: &mut Ctx<'_>, config: tkm_decide::config::DecideConfig) -> Result<(), CliError> {
    let env = ctx.env.clone();
    let var = |name: &str| {
        env.iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.clone())
    };
    let home = var("HOME")
        .map(PathBuf::from)
        .ok_or(CliError::PathsUnavailable)?;
    let key = match key::load_key(
        var("OPENROUTER_API_KEY"),
        &key::default_key_file(&home),
        None,
    ) {
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
            InMemoryLedger::default(),
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
        let available = runtime.block_on(async {
            if model.free_probe || model.slug.ends_with(":free") {
                client
                    .reprobe_free(request)
                    .await
                    .into_iter()
                    .any(|o| matches!(o, Outcome::Decision(_)))
            } else {
                matches!(client.decide(request).await, Outcome::Decision(_))
            }
        });
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
