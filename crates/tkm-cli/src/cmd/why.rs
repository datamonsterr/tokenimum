//! Explain a stored action using code-owned core templates and evidence.
use super::status::report;
use crate::{cli::WhyArgs, ctx::Ctx, error::CliError};
use std::process::ExitCode;
use tkm_core::explain::{self, ActionKind, ActionParams, Explanation, Params};
use tkm_decide::redact::Redactor;
use tkm_store::{EventRecord, SessionRecord, Store};

/// Execute `tkm why` with optional native or local session filtering.
///
/// # Errors
/// Returns an error when reading evidence or writing the explanation fails.
pub fn run(ctx: &mut Ctx<'_>, args: &WhyArgs) -> Result<ExitCode, CliError> {
    let store = report::read_store(ctx)?;
    let events = if args.event == "last" {
        store.list_recent_events(-1)
    } else if let Ok(id) = args.event.parse::<i64>() {
        store.get_event(id).map(|e| e.into_iter().collect())
    } else {
        Ok(Vec::new())
    }
    .map_err(|e| report::store_error(ctx, e))?;
    for event in events {
        if action_kind(&event).is_none() {
            continue;
        }
        let Some(session) = store
            .get_session(event.session_id)
            .map_err(|e| report::store_error(ctx, e))?
        else {
            continue;
        };
        if session.repo_key != ctx.repo.key
            || args
                .session
                .as_ref()
                .is_some_and(|id| id != &session.key.session_id && id != &session.id.to_string())
        {
            continue;
        }
        let explanation = explanation(&store, &event, &session, &report::redactor(ctx))
            .map_err(|e| report::store_error(ctx, e))?;
        if let Some(explanation) = explanation {
            let rendered = explain::render(&explanation);
            writeln!(ctx.stdout, "{rendered}")?;
            return Ok(ExitCode::SUCCESS);
        }
    }
    writeln!(
        ctx.stderr,
        "tkm why: unknown action event {}{}",
        report::line(&args.event),
        args.session
            .as_ref()
            .map_or_else(String::new, |s| format!(" in session {}", report::line(s)))
    )?;
    Ok(ExitCode::FAILURE)
}

pub(crate) fn action_kind(event: &EventRecord) -> Option<ActionKind> {
    serde_json::from_value(serde_json::Value::String(
        event.explanation_template_id.clone(),
    ))
    .ok()
    .or_else(|| {
        let template = match event.kind.as_str() {
            "fact.injected" => "fact_injected",
            "fact.promoted" => "fact_promoted",
            "fact.retired" => "fact_retired",
            "loop.redirect" | "loop.shadow" => "redirect",
            "loop.escalation" => "escalation",
            "prune.proposal" => "deny_list_proposal",
            "prune.write" => "deny_list_write",
            "prune.restore" => "restore",
            "budget.reached" => "budget_reached",
            "adapter.disabled" => "adapter_disabled",
            _ => return None,
        };
        serde_json::from_value(serde_json::Value::String(template.into())).ok()
    })
}

#[expect(
    clippy::too_many_lines,
    reason = "assemble one typed explanation from persisted evidence"
)]
pub(crate) fn explanation(
    store: &Store,
    event: &EventRecord,
    session: &SessionRecord,
    redactor: &Redactor,
) -> Result<Option<Explanation>, tkm_store::StoreError> {
    let Some(kind) = action_kind(event) else {
        return Ok(None);
    };
    let values = event.params.get("values").unwrap_or(&event.params);
    let mut params = Params::default();
    for (name, target) in [
        ("detail", &mut params.detail),
        ("why", &mut params.why),
        ("model_checks", &mut params.model_checks),
        ("cost", &mut params.cost),
        ("undo", &mut params.undo),
    ] {
        values
            .get(name)
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .clone_into(target);
        *target = redactor.redact(target);
    }
    params.event = format!(
        "Event {}, {}, {}, repo {}",
        event.id,
        report::time(event.created_at),
        report::host_name(session.key.host),
        redactor.redact(&session.repo_key.0)
    );
    let mut checks = Vec::new();
    let mut cost = 0.0;
    let mut decisions = 0;
    let mut unknown_cost = false;
    // New rows identify decision references separately because fact rowids share the integer namespace.
    let decision_ids = event
        .inputs
        .get("decision_ids")
        .and_then(serde_json::Value::as_array)
        .map_or_else(
            || event.evidence_ids.clone(),
            |ids| ids.iter().filter_map(serde_json::Value::as_i64).collect(),
        );
    for id in &decision_ids {
        if let Some(decision) = store.get_decision(*id)? {
            if decision.session_id.is_some_and(|id| id != session.id)
                || decision
                    .repo_key
                    .as_ref()
                    .is_some_and(|repo| repo != &session.repo_key)
            {
                continue;
            }
            decisions += 1;
            if let Some(value) = decision.cost_usd {
                cost += value;
            } else {
                unknown_cost = true;
            }
            let probability = |name: &str| {
                decision
                    .answers
                    .get(name)
                    .or_else(|| {
                        event
                            .inputs
                            .get("fact_ids")
                            .and_then(serde_json::Value::as_array)
                            .and_then(|ids| {
                                ids.iter()
                                    .filter_map(serde_json::Value::as_str)
                                    .find_map(|id| decision.answers.get(format!("{name}_{id}")))
                            })
                    })
                    .and_then(|a| {
                        a.get("noul")
                            .or_else(|| a.get("probability"))
                            .or_else(|| a.get("p_true"))
                    })
                    .and_then(serde_json::Value::as_f64)
            };
            if let (Some(stable), Some(safe)) = (probability("stable"), probability("safe")) {
                checks.push(format!(
                    "{} rated it stable ({stable:.2}) and safe ({safe:.2}).",
                    redactor.redact(&decision.model)
                ));
            } else {
                checks.push(format!(
                    "{} ({})",
                    redactor.redact(&decision.model),
                    redactor.redact(&decision.purpose)
                ));
            }
        }
    }
    if !checks.is_empty() {
        params.model_checks = checks.join("; ");
    }
    if decisions > 0 {
        params.cost = if unknown_cost {
            "unknown".into()
        } else {
            format!("${cost:.6}")
        };
    } else if params.cost.is_empty() {
        params.cost = "unknown".into();
    }
    if params.model_checks.is_empty() {
        params.model_checks = "no stored model checks".into();
    }
    if let Some(ids) = event
        .inputs
        .get("fact_ids")
        .and_then(serde_json::Value::as_array)
    {
        for id in ids.iter().filter_map(serde_json::Value::as_str) {
            if let Some(fact) = store.get_fact(&session.repo_key, id)? {
                if params.detail.is_empty() {
                    params.detail = super::facts::claim(&fact, redactor);
                }
                if params.why.is_empty() {
                    params.why = format!(
                        "the fact was confirmed in {} sessions and contradicted in {}; it fired in {} of {} sessions",
                        fact.confirmations,
                        fact.contradictions,
                        fact.fire_count,
                        fact.fire_sessions
                    );
                }
                if params.undo.is_empty()
                    && matches!(kind, ActionKind::FactInjected | ActionKind::FactPromoted)
                {
                    params.undo = format!("tkm facts forget {}", redactor.redact(id));
                }
            }
        }
    }
    if params.detail.is_empty() {
        params.detail.clone_from(&event.kind);
        params.detail = redactor.redact(&params.detail);
    }
    if params.why.is_empty() {
        params.why = "no additional evidence was recorded".into();
    }
    if params.undo.is_empty() {
        params.undo = "no undo action is available".into();
    }
    let params = match kind {
        ActionKind::FactInjected => ActionParams::FactInjected(explain::FactInjectedParams(params)),
        ActionKind::FactPromoted => ActionParams::FactPromoted(explain::FactPromotedParams(params)),
        ActionKind::FactRetired => ActionParams::FactRetired(explain::FactRetiredParams(params)),
        ActionKind::Redirect => ActionParams::Redirect(explain::RedirectParams(params)),
        ActionKind::Escalation => ActionParams::Escalation(explain::EscalationParams(params)),
        ActionKind::DenyListProposal => {
            ActionParams::DenyListProposal(explain::DenyListProposalParams(params))
        }
        ActionKind::DenyListWrite => {
            ActionParams::DenyListWrite(explain::DenyListWriteParams(params))
        }
        ActionKind::Restore => ActionParams::Restore(explain::RestoreParams(params)),
        ActionKind::BudgetReached => {
            ActionParams::BudgetReached(explain::BudgetReachedParams(params))
        }
        ActionKind::AdapterDisabled => {
            ActionParams::AdapterDisabled(explain::AdapterDisabledParams(params))
        }
    };
    Ok(Some(Explanation {
        template_id: kind,
        params,
    }))
}
