//! Fail-open daily model maintenance, outside hook handlers and fold workers.

use std::sync::Arc;
use std::time::Duration;

use tkm_core::model::Timestamp;
use tkm_decide::client::{NoDecisionReason, Outcome};
use tkm_decide::probe::FreeProbeResult;
use tkm_store::{FreeModelProbe, NewDecision, Store, StoreError};

use crate::daemon::{Daemon, lock};

fn revision(rows: &[FreeModelProbe]) -> Option<i64> {
    rows.iter().map(|row| row.probed_at).max()
}

fn persist(
    store: &Store,
    results: &[FreeProbeResult],
    now: i64,
) -> Result<Vec<FreeModelProbe>, StoreError> {
    let questions =
        serde_json::to_value(tkm_decide::probe::availability_request("tkmd-free-probe").questions)?;
    for result in results {
        match &result.outcome {
            Outcome::Decision(decision) => {
                store.record_free_model_probe(&result.model, true, now)?;
                store.insert_decision_with_context(
                    &NewDecision {
                        model: decision.model_version.clone(),
                        purpose: "free_model_probe".into(),
                        questions: questions.clone(),
                        answers: serde_json::to_value(&decision.answers)?,
                        cost_usd: decision.usage.cost,
                        latency_ms: u32::try_from(decision.latency_ms).unwrap_or(u32::MAX),
                        shadow: true,
                    },
                    Some(Timestamp(now)),
                    None,
                    None,
                    None,
                )?;
            }
            Outcome::NoDecision(
                NoDecisionReason::AllFailed | NoDecisionReason::RequestRejected,
            ) => {
                store.record_free_model_probe(&result.model, false, now)?;
            }
            // Offline, disabled and over-budget paths did not test availability.
            _ => {}
        }
    }
    store.free_model_probes()
}

impl Daemon {
    /// Run model maintenance if the durable daily timestamp is due. Every error is logged and
    /// returns normally; live requests continue on the existing chain. A successful free probe
    /// promotes the slot on the very same decision client used by hooks.
    pub async fn reprobe_models(self: &Arc<Self>) {
        let _maintenance = self.maintenance_lock.lock().await;
        if !self.default_settings.decide.enabled || self.decider.free_models().is_empty() {
            return;
        }
        let now = i64::try_from(self.clock.now_ms()).unwrap_or(i64::MAX);
        let backing = self.backing.clone();
        let snapshot = tokio::task::spawn_blocking(move || {
            let store = backing.open()?;
            let rows = store.free_model_probes()?;
            let claimed = store.claim_free_model_probe(now)?;
            Ok::<_, StoreError>((rows, claimed))
        })
        .await;
        let (rows, claimed) = match snapshot {
            Ok(Ok(snapshot)) => snapshot,
            Ok(Err(error)) => {
                self.log_error(&format!("free model probe storage: {error}"));
                return;
            }
            Err(error) => {
                self.log_error(&format!("free model probe worker: {error}"));
                return;
            }
        };
        let current = revision(&rows);
        {
            let mut last = lock(&self.model_probe_revision);
            if *last != current {
                for row in rows {
                    self.decider.restore_free_model(&row.model, row.succeeded);
                }
                *last = current;
            }
        }
        if !claimed {
            return;
        }
        // Include the day in the session budget key: yesterday's probe cannot exhaust today's
        // per-session allowance, while every attempt still counts against today's daily budget.
        let results = self
            .decider
            .reprobe_free(format!("tkmd-free-probe:{}", now / 86_400_000))
            .await;
        for result in &results {
            if matches!(result.outcome, Outcome::NoDecision(_)) {
                self.log_error(&format!("free model probe unavailable: {}", result.model));
            }
        }
        let backing = self.backing.clone();
        match tokio::task::spawn_blocking(move || persist(&backing.open()?, &results, now)).await {
            Ok(Ok(rows)) => *lock(&self.model_probe_revision) = revision(&rows),
            Ok(Err(error)) => self.log_error(&format!("free model probe persistence: {error}")),
            Err(error) => self.log_error(&format!("free model probe worker: {error}")),
        }
    }

    pub(crate) async fn maintenance_loop(self: Arc<Self>) {
        let mut repriced = false;
        loop {
            // Health changes made by doctor apply to sessions already running. Read outside
            // state locks and never on the tool hot path.
            let daemon = self.clone();
            match tokio::task::spawn_blocking(move || {
                let hosts: std::collections::HashSet<_> = lock(&daemon.state)
                    .sessions
                    .values()
                    .map(|session| session.host)
                    .collect();
                if hosts.is_empty() {
                    return Ok(());
                }
                let store = daemon.backing.open()?;
                for host in hosts {
                    daemon.refresh_host_health(&store, host)?;
                }
                Ok::<_, StoreError>(())
            })
            .await
            {
                Ok(Ok(())) => {}
                Ok(Err(error)) => self.log_error(&format!("host health refresh: {error}")),
                Err(error) => self.log_error(&format!("host health worker: {error}")),
            }
            self.reprobe_models().await;
            // Once per start, after the probe, so the two never contend for the store.
            if !repriced {
                self.reprice_ledger().await;
                repriced = true;
            }
            tokio::time::sleep(Duration::from_secs(60)).await;
        }
    }
}
