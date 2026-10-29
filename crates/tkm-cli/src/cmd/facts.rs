//! Fact evidence and lifecycle retirement.
use super::status::report;
use crate::{
    cli::{FactsArgs, FactsCommand},
    ctx::Ctx,
    daemon,
    error::CliError,
};
use std::process::ExitCode;
use tkm_core::facts::{Candidate, Family, Params, Scope};
use tkm_core::model::Timestamp;
use tkm_store::{FactRecord, FactState};

/// Execute `tkm facts` or retire a repository fact.
///
/// # Errors
/// Returns an error when the database, configuration or output cannot be accessed.
pub fn run(ctx: &mut Ctx<'_>, args: &FactsArgs) -> Result<ExitCode, CliError> {
    if let Some(FactsCommand::Forget(args)) = &args.command {
        let store = daemon::open_db_for_write(&ctx.paths)?;
        let Some(fact) = store
            .get_fact(&ctx.repo.key, &args.id)
            .map_err(|e| report::store_error(ctx, e))?
        else {
            writeln!(
                ctx.stderr,
                "tkm facts: unknown fact {}",
                report::line(&args.id)
            )?;
            return Ok(ExitCode::FAILURE);
        };
        if fact.state != FactState::Retired {
            store
                .transition_fact_at(
                    &ctx.repo.key,
                    &args.id,
                    FactState::Retired,
                    Timestamp(report::now_ms()),
                )
                .map_err(|e| report::store_error(ctx, e))?;
        }
        writeln!(
            ctx.stdout,
            "Retired fact {}. This cannot be undone.",
            report::line(&args.id)
        )?;
        return Ok(ExitCode::SUCCESS);
    }
    let store = report::read_store(ctx)?;
    let facts = store
        .list_facts(
            &ctx.repo.key,
            if args.all {
                None
            } else {
                Some(FactState::Active)
            },
        )
        .map_err(|e| report::store_error(ctx, e))?;
    if facts.is_empty() && !args.brief {
        writeln!(
            ctx.stdout,
            "No {}facts for {}.",
            if args.all { "" } else { "active " },
            report::line(&ctx.repo.key.0)
        )?;
    }
    let redactor = report::redactor(ctx);
    for fact in facts {
        let claim = claim(&fact);
        if args.brief {
            let rendered = redactor.redact(&format!("{}: {claim}", report::line(&fact.fact_id)));
            writeln!(ctx.stdout, "{rendered}")?;
        } else {
            let rendered = redactor.redact(&format!(
                "{} [{}]: {claim}",
                report::line(&fact.fact_id),
                fact.state.as_str()
            ));
            writeln!(ctx.stdout, "{rendered}")?;
            writeln!(
                ctx.stdout,
                "  Evidence: {} confirmations, {} contradictions; fired {}/{} sessions; state changed {}",
                fact.confirmations,
                fact.contradictions,
                fact.fire_count,
                fact.fire_sessions,
                report::time(fact.state_changed_at)
            )?;
        }
    }
    Ok(ExitCode::SUCCESS)
}

#[must_use]
pub(crate) fn claim(fact: &FactRecord) -> String {
    let family = serde_json::from_value::<Family>(serde_json::Value::String(fact.family.clone()));
    let params = fact
        .text_params
        .get("params")
        .unwrap_or(&fact.text_params)
        .clone();
    let params = serde_json::from_value::<Params>(params);
    let scope = serde_json::from_str::<Scope>(&fact.scope)
        .or_else(|_| serde_json::from_value(serde_json::Value::String(fact.scope.clone())));
    if let (Ok(family), Ok(params), Ok(scope)) = (family, params, scope) {
        let candidate = Candidate {
            id: fact.fact_id.clone(),
            family,
            params,
            scope,
            evidence: Vec::new(),
        };
        let rendered = tkm_core::facts::render::claim(&candidate);
        if !rendered.is_empty() {
            return rendered;
        }
    }
    format!(
        "{} (stored evidence; claim unavailable)",
        report::line(&fact.family)
    )
}
