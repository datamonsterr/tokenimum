//! Decision client wiring: an object-safe [`Decider`], the spend ledger and the calibration
//! store the client needs (ADR 0013), and the factory used by the binary.

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex, PoisonError};

use tkm_decide::budget::{DecisionRow, InMemoryLedger, SpendLedger};
use tkm_decide::calibrate::{
    CalibrationKey, CalibrationStore, MIN_LABELS, Thresholds, precision_target,
};
use tkm_decide::client::{
    DecideRequest, DecisionClient, HttpTransport, NoDecisionReason, Outcome, Transport,
};
use tkm_decide::config::DecideConfig;
use tkm_decide::key::ApiKey;
use tkm_integrations::{Escalator, superorchestrator::SpendSink};
use tkm_store::CalibrationRecord;

use crate::clock::SystemClock;

/// A boxed, sendable future.
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// Object-safe decision source. The client's own transport trait is not object-safe, so the
/// daemon holds this instead and tests substitute scripted deciders.
pub trait Decider: Send + Sync {
    /// Runs one batched decision request. Never fails: failures are outcomes.
    fn decide(&self, request: DecideRequest) -> BoxFuture<'_, Outcome>;
    /// Probe free slots on the same live client. Sources without models do nothing.
    fn reprobe_free(
        &self,
        _session_id: String,
    ) -> BoxFuture<'_, Vec<tkm_decide::probe::FreeProbeResult>> {
        Box::pin(async { Vec::new() })
    }
    /// Configured free slots; empty for offline or scripted sources.
    fn free_models(&self) -> Vec<String> {
        Vec::new()
    }
    /// Current available model order, useful for diagnostics.
    fn active_models(&self) -> Vec<String> {
        Vec::new()
    }
    /// Restore one persisted result after daemon restart or an explicit doctor probe.
    fn restore_free_model(&self, _model: &str, _succeeded: bool) {}
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
pub struct ClientDecider<T: Transport, C: crate::clock::Clock = SystemClock> {
    client: DecisionClient<T, C, Arc<StoreLedger>>,
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

impl<T: Transport, C: crate::clock::Clock> ClientDecider<T, C> {
    /// Create a client with an injected clock for maintenance and budget tests.
    pub fn with_clock(
        config: DecideConfig,
        key: Option<ApiKey>,
        transport: T,
        clock: C,
        ledger: Arc<StoreLedger>,
    ) -> Self {
        Self {
            client: DecisionClient::new(config, key, transport, clock, ledger),
        }
    }
}

impl<T: Transport + 'static, C: crate::clock::Clock + 'static> Decider for ClientDecider<T, C> {
    fn decide(&self, request: DecideRequest) -> BoxFuture<'_, Outcome> {
        Box::pin(async move { self.client.decide(request).await })
    }
    fn reprobe_free(
        &self,
        session_id: String,
    ) -> BoxFuture<'_, Vec<tkm_decide::probe::FreeProbeResult>> {
        Box::pin(async move { self.client.probe_free_models(&session_id).await })
    }
    fn free_models(&self) -> Vec<String> {
        self.client.free_models()
    }
    fn active_models(&self) -> Vec<String> {
        self.client.active_models()
    }
    fn restore_free_model(&self, model: &str, succeeded: bool) {
        self.client.restore_free_model(model, succeeded);
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

type SpendTarget = (Arc<dyn SpendSink>, Arc<dyn Escalator>);
enum SpendMessage {
    Record(Arc<dyn SpendSink>, String, String, f64, Option<String>),
    Flush(Option<Arc<dyn SpendSink>>, std::sync::mpsc::Sender<()>),
    Attempt(std::path::PathBuf, DecisionRow),
}

/// Spend ledger for the decision client. Rows live in memory (cap checks must not touch disk);
/// each decision is also written to the `decisions` table by the daemon through the outbox,
/// where it carries the answers and the shadow flag the client row lacks.
pub struct StoreLedger {
    inner: InMemoryLedger,
    sink: Mutex<Option<SpendTarget>>,
    spend_tx: std::sync::mpsc::Sender<SpendMessage>,
    persistence: Mutex<Option<std::path::PathBuf>>,
}

impl Default for StoreLedger {
    fn default() -> Self {
        let (spend_tx, rx) = std::sync::mpsc::channel::<SpendMessage>();
        std::thread::spawn(move || {
            while let Ok(message) = rx.recv() {
                match message {
                    SpendMessage::Record(sink, purpose, model, cost, run_id) => {
                        sink.record(&purpose, &model, cost, run_id.as_deref());
                    }
                    SpendMessage::Attempt(path, row) => {
                        let result = tkm_store::Store::open(&path).and_then(|store| {
                            let value = serde_json::to_value(row)?;
                            store.insert_spend_attempt(&value)
                        });
                        if let Err(error) = result {
                            tracing::warn!(%error, "could not persist decision spend");
                        }
                    }
                    SpendMessage::Flush(sink, ack) => {
                        if let Some(sink) = sink {
                            sink.flush();
                        }
                        let _ = ack.send(());
                    }
                }
            }
        });
        Self {
            inner: InMemoryLedger::default(),
            sink: Mutex::new(None),
            spend_tx,
            persistence: Mutex::new(None),
        }
    }
}

impl StoreLedger {
    /// Restore previously persisted decisions before serving requests.
    ///
    /// # Errors
    /// Returns an error if the stored decisions or sessions cannot be read.
    pub fn restore(&self, store: &tkm_store::Store) -> Result<(), tkm_store::StoreError> {
        let legacy_max = store.legacy_spend_max_id()?;
        for decision in store.list_recent_decisions(-1)?.into_iter().filter(|d| {
            d.id <= legacy_max || matches!(d.purpose.as_str(), "models_probe" | "doctor_probe")
        }) {
            let Some(ts_ms) = decision.created_at.and_then(|t| u64::try_from(t.0).ok()) else {
                continue;
            };
            let session = decision
                .session_id
                .map(|id| store.get_session(id))
                .transpose()?
                .flatten();
            let session_id = session.map_or_else(
                || format!("legacy:{}", decision.id),
                |session| format!("{}:{}", session.key.host.as_str(), session.key.session_id),
            );
            let purpose = match decision.purpose.as_str() {
                "loop_guard" => tkm_decide::cache::Purpose::LoopGuard,
                "relevance" | "models_probe" | "doctor_probe" => {
                    tkm_decide::cache::Purpose::Relevance
                }
                "fact_filter" => tkm_decide::cache::Purpose::FactFilter,
                "fact_contradiction" => tkm_decide::cache::Purpose::FactContradiction,
                "redirect" => tkm_decide::cache::Purpose::Redirect,
                _ => continue,
            };
            self.inner.record(DecisionRow {
                ts_ms,
                session_id,
                purpose,
                requested_model: decision.model.clone(),
                model_version: Some(decision.model),
                provider: None,
                input_tokens: 0,
                output_tokens: 0,
                cost_usd: decision.cost_usd,
                latency_ms: u64::from(decision.latency_ms),
                question_names: Vec::new(),
                outcome: "restored".into(),
                cached: false,
            });
        }
        for value in store.list_spend_attempts()? {
            let row = serde_json::from_value(value)?;
            self.inner.record(row);
        }
        Ok(())
    }

    /// Load and enable durable attempt storage before serving requests.
    ///
    /// # Errors
    /// Returns an error if the database cannot be opened or restored.
    pub fn enable_persistence(
        &self,
        path: std::path::PathBuf,
    ) -> Result<(), tkm_store::StoreError> {
        let store = tkm_store::Store::open(&path)?;
        self.restore(&store)?;
        *self
            .persistence
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = Some(path);
        Ok(())
    }

    /// Count attempts today for which the cost was unavailable.
    pub fn unknown_today(&self, now_ms: u64) -> usize {
        self.inner
            .rows()
            .iter()
            .filter(|row| row.ts_ms / 86_400_000 == now_ms / 86_400_000 && row.cost_usd.is_none())
            .count()
    }

    /// Waits up to two seconds for previously queued spend records.
    pub fn flush(&self) {
        let (tx, rx) = std::sync::mpsc::channel();
        let sink = self
            .sink
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .as_ref()
            .map(|(sink, _)| Arc::clone(sink));
        if self.spend_tx.send(SpendMessage::Flush(sink, tx)).is_ok() {
            let _ = rx.recv_timeout(std::time::Duration::from_secs(2));
        }
    }
    /// Enables reporting to an integration sink while the adapter is active.
    pub fn set_spend_sink(&self, sink: Arc<dyn SpendSink>, escalator: Arc<dyn Escalator>) {
        *self.sink.lock().unwrap_or_else(PoisonError::into_inner) = Some((sink, escalator));
    }
    /// Tokens and cost the daemon spent on decisions for one session id.
    pub fn session_overhead(&self, session_id: &str) -> (u64, Option<f64>) {
        self.inner
            .rows()
            .iter()
            .filter(|row| row.session_id == session_id)
            .fold((0, Some(0.0)), |(tokens, cost), row| {
                (
                    tokens + row.input_tokens + row.output_tokens,
                    cost.zip(row.cost_usd).map(|(total, amount)| total + amount),
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
        let path = self
            .persistence
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        if let Some(path) = path {
            let _ = self.spend_tx.send(SpendMessage::Attempt(path, row.clone()));
        }
        let target = self
            .sink
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        if !row.cached
            && let Some(cost) = row.cost_usd
            && let Some((sink, escalator)) = target
            && escalator.superorchestrator_enabled()
        {
            let run_id = row.session_id.split_once(':').and_then(|(host, session)| {
                host.parse::<tkm_proto::Host>()
                    .ok()
                    .and_then(|h| escalator.run_id(h, session))
            });
            let _ = self.spend_tx.send(SpendMessage::Record(
                sink,
                row.purpose.as_str().to_owned(),
                row.model_version
                    .as_deref()
                    .unwrap_or(&row.requested_model)
                    .to_owned(),
                cost,
                run_id,
            ));
        }
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
        values.clear();
        for record in records {
            if !calibration_qualified(record) {
                continue;
            }
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

/// Whether measured quality permits this exact model/purpose to leave shadow.
#[must_use]
pub fn calibration_qualified(record: &CalibrationRecord) -> bool {
    precision_target(&record.purpose)
        .is_some_and(|target| (target..=1.0).contains(&record.precision))
        && usize::try_from(record.sample_count).is_ok_and(|n| n >= MIN_LABELS)
        && (tkm_decide::calibrate::FitConfig::default().min_recall..=1.0).contains(&record.recall)
}

/// Initial observation window for one purpose; another purpose cannot satisfy it.
/// Database failures retain shadow. Duplicate requests for one session/digest count once.
#[must_use]
pub fn purpose_shadow(
    store: &tkm_store::Store,
    purpose: &str,
    now: i64,
    days: u64,
    turns: u64,
) -> bool {
    if days == 0 && turns == 0 {
        return false;
    }
    let Ok(rows) = store.list_recent_decisions(-1) else {
        return true;
    };
    let mut start = now;
    let mut judged = std::collections::BTreeSet::new();
    for row in rows.iter().filter(|row| row.purpose == purpose) {
        if let Some(at) = row.created_at {
            start = start.min(at.0);
            judged.insert((row.session_id, row.digest.as_deref()));
        }
    }
    now.saturating_sub(start) < i64::try_from(days.saturating_mul(86_400_000)).unwrap_or(i64::MAX)
        || u64::try_from(judged.len()).unwrap_or(u64::MAX) < turns
}

fn thresholds_from_json(value: &serde_json::Value) -> Option<Thresholds> {
    let get = |name: &str| value.get(name).and_then(serde_json::Value::as_f64);
    let thresholds = Thresholds {
        t_stuck: get("t_stuck")?,
        t_progress: get("t_progress")?,
        t_low: get("t_low")?,
        t_high: get("t_high")?,
    };
    ([
        thresholds.t_stuck,
        thresholds.t_progress,
        thresholds.t_low,
        thresholds.t_high,
    ]
    .iter()
    .all(|v| (0.0..=1.0).contains(v))
        && thresholds.t_low <= thresholds.t_high
        && thresholds.t_stuck > 0.0)
        .then_some(thresholds)
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
        // An unscored row cannot authorize any decision purpose.
        self.values
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(&key);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn put_is_queued_without_authorizing_unscored_loop() {
        let store = StoreCalibration::default();
        let key = CalibrationKey {
            model_version: "m-20260101".into(),
            purpose: "loop_guard".into(),
        };
        assert!(store.get(&key).is_none());
        store.put(key.clone(), Thresholds::default());
        assert_eq!(store.get(&key), None);
        let pending = store.take_pending();
        assert_eq!(pending.len(), 1);
        assert_eq!(store.take_pending().len(), 0);
    }

    #[test]
    fn refresh_requires_loop_precision_and_revokes_stale_authorization() {
        let store = StoreCalibration::default();
        let key = CalibrationKey {
            model_version: "m".into(),
            purpose: "loop_guard".into(),
        };
        let mut row = CalibrationRecord {
            model_version: key.model_version.clone(),
            purpose: key.purpose.clone(),
            thresholds: serde_json::json!({"t_stuck":0.9,"t_progress":0.2,"t_low":0.1,"t_high":0.8}),
            sample_count: 200,
            precision: 0.9,
            recall: 0.6,
        };
        store.refresh(&[row.clone()]);
        assert!(store.get(&key).is_some());
        row.precision = 0.899;
        store.refresh(&[row]);
        assert!(store.get(&key).is_none());
        store.refresh(&[]);
        assert!(store.get(&key).is_none());
    }

    #[test]
    fn unscored_put_does_not_authorize_loop_action() {
        let store = StoreCalibration::default();
        let key = CalibrationKey {
            model_version: "m".into(),
            purpose: "loop_guard".into(),
        };
        store.put(key.clone(), Thresholds::default());
        assert!(store.get(&key).is_none());
    }

    #[test]
    fn refresh_loads_rows_from_the_database() {
        let store = StoreCalibration::default();
        store.refresh(&[CalibrationRecord {
            model_version: "m".into(),
            purpose: "relevance".into(),
            thresholds: serde_json::json!({"t_stuck":0.9,"t_progress":0.2,"t_low":0.1,"t_high":0.8}),
            sample_count: 200,
            precision: 0.95,
            recall: 0.6,
        }]);
        let found = store.get(&CalibrationKey {
            model_version: "m".into(),
            purpose: "relevance".into(),
        });
        assert_eq!(found.map(|t| t.t_stuck), Some(0.9));
    }
}
