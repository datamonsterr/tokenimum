//! Explicit probes use the decision client's privacy, deadlines, and budget gates.

use std::collections::BTreeMap;
use std::sync::Arc;

use tkm_config::config::EffectiveConfig;
use tkm_decide::api::Question;
use tkm_decide::budget::InMemoryLedger;
use tkm_decide::cache::Purpose;
use tkm_decide::chain::SystemClock;
use tkm_decide::client::{DecideRequest, DecisionClient, HttpTransport, Outcome};
use tkm_decide::key::ApiKey;

use super::{line, state};
use crate::{ctx::Ctx, daemon, error::CliError};

#[allow(clippy::too_many_lines)] // The probe worker and its persisted attempt report form one workflow.
pub(super) fn probe(
    ctx: &mut Ctx<'_>,
    config: &EffectiveConfig,
    key: ApiKey,
) -> Result<bool, CliError> {
    let decide = tkmd::settings::Settings::from_config(config).decide;
    let endpoint = state::env(ctx, "TKM_DECIDE_ENDPOINT").map(str::to_owned);
    let questions = BTreeMap::from([(
        "available".to_owned(),
        Question::Noul {
            instructions: "Is the probe marker equal to tokenimum?".into(),
            criteria: None,
        },
    )]);
    let request = DecideRequest {
        purpose: Purpose::Relevance,
        session_id: "tkm-doctor-probe".into(),
        state: "{\"probe_marker\":\"tokenimum\"}".into(),
        questions: questions.clone(),
    };
    let worker = std::thread::Builder::new()
        .name("tkm-model-probe".into())
        .spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_io()
                .enable_time()
                .build()
                .map_err(state::failure)?;
            let ledger = Arc::new(InMemoryLedger::default());
            let mut outcomes = Vec::new();
            for model in &decide.chain {
                let mut one = decide.clone();
                one.chain = vec![model.clone()];
                let transport = match &endpoint {
                    Some(url) => HttpTransport::with_endpoint(url),
                    None => HttpTransport::new(),
                }
                .map_err(state::failure)?;
                let client = DecisionClient::new(
                    one,
                    Some(key.clone()),
                    transport,
                    SystemClock,
                    Arc::clone(&ledger),
                );
                let outcome = if model.free_probe || model.slug.ends_with(":free") {
                    runtime
                        .block_on(client.reprobe_free(request.clone()))
                        .into_iter()
                        .next()
                } else {
                    Some(runtime.block_on(client.decide(request.clone())))
                };
                outcomes.push((model.slug.clone(), outcome));
            }
            Ok::<_, CliError>((outcomes, ledger.rows()))
        })?;
    let (outcomes, attempts) = match worker.join() {
        Ok(Ok(result)) => result,
        _ => {
            line(ctx, "fail", "model probes", "probe worker failed")?;
            return Ok(false);
        }
    };
    let store = daemon::open_db_for_write(&ctx.paths)?;
    let questions = serde_json::to_value(questions).map_err(state::failure)?;
    for attempt in attempts {
        let answers = outcomes
            .iter()
            .find_map(|(_, outcome)| match outcome {
                Some(Outcome::Decision(decision))
                    if decision.requested_model == attempt.requested_model =>
                {
                    serde_json::to_value(&decision.answers).ok()
                }
                _ => None,
            })
            .unwrap_or_else(|| serde_json::json!({}));
        store
            .insert_decision(&tkm_store::NewDecision {
                model: attempt.model_version.unwrap_or(attempt.requested_model),
                purpose: "doctor_probe".into(),
                questions: questions.clone(),
                answers,
                cost_usd: attempt.cost_usd.unwrap_or(0.0),
                latency_ms: u32::try_from(attempt.latency_ms).unwrap_or(u32::MAX),
                shadow: true,
            })
            .map_err(state::failure)?;
    }
    let mut healthy = !outcomes.is_empty();
    for (model, outcome) in outcomes {
        let ok = matches!(outcome, Some(Outcome::Decision(_)));
        healthy &= ok;
        line(
            ctx,
            if ok { "ok" } else { "fail" },
            "model probes",
            &format!(
                "{model}: {}",
                if ok {
                    "available"
                } else {
                    "unavailable or budget exhausted"
                }
            ),
        )?;
    }
    Ok(healthy)
}
