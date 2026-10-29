//! Decision client wiring: an object-safe [`Decider`], the spend ledger and the calibration
//! store the client needs (ADR 0013), and the factory used by the binary.

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex, PoisonError};

use tkm_decide::budget::{DecisionRow, InMemoryLedger, SpendLedger};
use tkm_decide::calibrate::{CalibrationKey, CalibrationStore, Thresholds};
use tkm_decide::client::{
    DecideRequest, DecisionClient, HttpTransport, NoDecisionReason, Outcome, Transport,
};
use tkm_decide::config::DecideConfig;
use tkm_decide::key::ApiKey;
use tkm_store::CalibrationRecord;

use crate::clock::SystemClock;

/// A boxed, sendable future.
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// Object-safe decision source. The client's own transport trait is not object-safe, so the
/// daemon holds this instead and tests substitute scripted deciders.
pub trait Decider: Send + Sync {
    /// Runs one batched decision request. Never fails: failures are outcomes.
    fn decide(&self, request: DecideRequest) -> BoxFuture<'_, Outcome>;
}

/// Decider that always reports "disabled" (decisions off, or no transport).
#[derive(Debug, Clone, Copy, Default)]
pub struct NullDecider;

impl Decider for NullDecider {
    fn decide(&self, _request: DecideRequest) -> BoxFuture<'_, Outcome> {
        Box::pin(async { Outcome::NoDecision(NoDecisionReason::Disabled) })
    }
}

/// [`Decider`] over the real [`DecisionClient`].
pub struct ClientDecider<T: Transport> {
    client: DecisionClient<T, SystemClock, Arc<StoreLedger>>,
}

impl<T: Transport + 'static> ClientDecider<T> {
    /// Wraps a transport with the budgeted, cached, model-chained client.
    pub fn new(
        config: DecideConfig,
        key: Option<ApiKey>,
        transport: T,
        ledger: Arc<StoreLedger>,
    ) -> Self {
        Self {
            client: DecisionClient::new(config, key, transport, SystemClock, ledger),
        }
    }
}

impl<T: Transport + 'static> Decider for ClientDecider<T> {
    fn decide(&self, request: DecideRequest) -> BoxFuture<'_, Outcome> {
        Box::pin(async move { self.client.decide(request).await })
    }
}

/// Builds the production decider. `endpoint` overrides the `OpenRouter` URL (tests and the
/// `TKM_DECIDE_ENDPOINT` hook); a transport that cannot be built degrades to [`NullDecider`].
pub fn build_decider(
    config: DecideConfig,
    key: Option<ApiKey>,
    endpoint: Option<&str>,
    ledger: Arc<StoreLedger>,
) -> Arc<dyn Decider> {
    let transport = match endpoint {
        Some(url) => HttpTransport::with_endpoint(url),
        None => HttpTransport::new(),
    };
    match transport {
        Ok(transport) => Arc::new(ClientDecider::new(config, key, transport, ledger)),
        Err(_) => Arc::new(NullDecider),
    }
}

/// Spend ledger for the decision client. Rows live in memory (cap checks must not touch disk);
/// each decision is also written to the `decisions` table by the daemon through the outbox,
/// where it carries the answers and the shadow flag the client row lacks.
#[derive(Debug, Default)]
pub struct StoreLedger {
    inner: InMemoryLedger,
}

impl StoreLedger {
    /// Tokens and cost the daemon spent on decisions for one session id.
    pub fn session_overhead(&self, session_id: &str) -> (u64, f64) {
        self.inner
            .rows()
            .iter()
            .filter(|row| row.session_id == session_id)
            .fold((0, 0.0), |(tokens, cost), row| {
                (
                    tokens + row.input_tokens + row.output_tokens,
                    cost + row.cost_usd.unwrap_or(0.0),
                )
            })
    }

    /// Spend since the start of the UTC day of `now_ms`.
    pub fn spent_today(&self, now_ms: u64) -> f64 {
        self.inner.spent_today_usd(now_ms)
    }
}

impl SpendLedger for StoreLedger {
    fn spent_today_usd(&self, now_ms: u64) -> f64 {
        self.inner.spent_today_usd(now_ms)
    }

    fn spent_session_usd(&self, session_id: &str) -> f64 {
        self.inner.spent_session_usd(session_id)
    }

    fn record(&self, row: DecisionRow) {
        self.inner.record(row);
    }
}

/// Calibration thresholds per (model version, purpose), refreshed from the database by the
/// flusher. `put` also queues the row for persistence.
#[derive(Debug, Default)]
pub struct StoreCalibration {
    values: Mutex<HashMap<CalibrationKey, Thresholds>>,
    pending: Mutex<Vec<CalibrationRecord>>,
}

impl StoreCalibration {
    /// Replaces cached thresholds with the rows read from the database.
    pub fn refresh(&self, records: &[CalibrationRecord]) {
        let mut values = self.values.lock().unwrap_or_else(PoisonError::into_inner);
        for record in records {
            if let Some(thresholds) = thresholds_from_json(&record.thresholds) {
                values.insert(
                    CalibrationKey {
                        model_version: record.model_version.clone(),
                        purpose: record.purpose.clone(),
                    },
                    thresholds,
                );
            }
        }
    }

    /// Takes the rows queued by [`CalibrationStore::put`].
    pub fn take_pending(&self) -> Vec<CalibrationRecord> {
        std::mem::take(&mut *self.pending.lock().unwrap_or_else(PoisonError::into_inner))
    }
}

fn thresholds_from_json(value: &serde_json::Value) -> Option<Thresholds> {
    let get = |name: &str| value.get(name).and_then(serde_json::Value::as_f64);
    Some(Thresholds {
        t_stuck: get("t_stuck")?,
        t_progress: get("t_progress")?,
        t_low: get("t_low")?,
        t_high: get("t_high")?,
    })
}

impl CalibrationStore for StoreCalibration {
    fn get(&self, key: &CalibrationKey) -> Option<Thresholds> {
        self.values
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(key)
            .copied()
    }

    fn put(&self, key: CalibrationKey, t: Thresholds) {
        self.pending
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(CalibrationRecord {
                model_version: key.model_version.clone(),
                purpose: key.purpose.clone(),
                thresholds: serde_json::json!({
                    "t_stuck": t.t_stuck,
                    "t_progress": t.t_progress,
                    "t_low": t.t_low,
                    "t_high": t.t_high,
                }),
                sample_count: 0,
                precision: 0.0,
                recall: 0.0,
            });
        self.values
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(key, t);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn put_is_visible_and_queued_for_persistence() {
        let store = StoreCalibration::default();
        let key = CalibrationKey {
            model_version: "m-20260101".into(),
            purpose: "loop_guard".into(),
        };
        assert!(store.get(&key).is_none());
        store.put(key.clone(), Thresholds::default());
        assert_eq!(store.get(&key), Some(Thresholds::default()));
        let pending = store.take_pending();
        assert_eq!(pending.len(), 1);
        assert_eq!(store.take_pending().len(), 0);
    }

    #[test]
    fn refresh_loads_rows_from_the_database() {
        let store = StoreCalibration::default();
        store.refresh(&[CalibrationRecord {
            model_version: "m".into(),
            purpose: "p".into(),
            thresholds: serde_json::json!({"t_stuck":0.9,"t_progress":0.2,"t_low":0.1,"t_high":0.8}),
            sample_count: 200,
            precision: 0.95,
            recall: 0.6,
        }]);
        let found = store.get(&CalibrationKey {
            model_version: "m".into(),
            purpose: "p".into(),
        });
        assert_eq!(found.map(|t| t.t_stuck), Some(0.9));
    }
}
