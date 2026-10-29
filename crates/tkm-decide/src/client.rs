//! Fail-open, budgeted Decisions client and injectable HTTP transport.
use crate::{
    api::{self, Answer, DecisionsRequest, DecisionsResponse, Question, Usage},
    budget::{Budget, BudgetVerdict, DecisionRow, SpendLedger},
    cache::{DecisionCache, Purpose},
    chain::{Clock, ModelChain, is_non_retryable},
    config::DecideConfig,
    key::ApiKey,
};
use std::{collections::BTreeMap, future::Future, sync::Arc, time::Duration};
/// Status and unparsed body of a Decisions HTTP reply.
#[derive(Clone, Debug)]
pub struct HttpReply {
    /// HTTP status code.
    pub status: u16,
    /// UTF-8 response body, never logged by the client.
    pub body: String,
}
/// Safe transport error category; intentionally excludes remote data and secrets.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum TransportError {
    /// HTTP client setup failed.
    #[error("HTTP client initialization failed")]
    Initialization,
    /// Request could not be delivered.
    #[error("HTTP request failed")]
    Request,
    /// Response body could not be read.
    #[error("HTTP response body failed")]
    Body,
}
/// Transport seam for fixture-only tests.
pub trait Transport: Send + Sync {
    /// Post the already-serialized request body.
    ///
    /// # Errors
    /// Returns a safe transport category on delivery or body-read failure.
    fn post_decision(
        &self,
        key: &ApiKey,
        body: String,
    ) -> impl Future<Output = Result<HttpReply, TransportError>> + Send;
}
/// Production `reqwest` transport to the fixed `OpenRouter` endpoint.
pub struct HttpTransport {
    client: reqwest::Client,
}
impl HttpTransport {
    /// Construct the reusable HTTP client without panicking on setup failures.
    ///
    /// # Errors
    /// Returns `TransportError::Initialization` when client setup fails.
    pub fn new() -> Result<Self, TransportError> {
        reqwest::Client::builder()
            .build()
            .map(|client| Self { client })
            .map_err(|_| TransportError::Initialization)
    }
}
impl Transport for HttpTransport {
    async fn post_decision(&self, key: &ApiKey, body: String) -> Result<HttpReply, TransportError> {
        let reply = self
            .client
            .post("https://openrouter.ai/api/alpha/decisions")
            .bearer_auth(key.expose_secret())
            .header("Content-Type", "application/json")
            .body(body)
            .send()
            .await
            .map_err(|_| TransportError::Request)?;
        let status = reply.status().as_u16();
        let body = reply.text().await.map_err(|_| TransportError::Body)?;
        Ok(HttpReply { status, body })
    }
}
/// A single purpose's already-built, already-redacted state and questions.
#[derive(Clone, Debug)]
pub struct DecideRequest {
    /// Purpose determines cache expiry and ledger grouping.
    pub purpose: Purpose,
    /// Full local session identifier.
    pub session_id: String,
    /// Redacted UTF-8 application state.
    pub state: String,
    /// All questions to batch into a single request per model.
    pub questions: BTreeMap<String, Question>,
}
/// Validated answer batch and its provenance.
#[derive(Clone, Debug, PartialEq)]
pub struct Decision {
    /// Actual response model version used by calibration.
    pub model_version: String,
    /// Requested configured model slug.
    pub requested_model: String,
    /// Serving provider, when present.
    pub provider: Option<String>,
    /// Validated typed answers.
    pub answers: BTreeMap<String, Answer>,
    /// Original API usage, also retained on cache hits.
    pub usage: Usage,
    /// Network elapsed time; zero for cache hits.
    pub latency_ms: u64,
    /// Whether this result came from the in-memory cache.
    pub cached: bool,
}
/// Reasons the caller must use code-only behavior.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CodeOnlyReason {
    /// No API key was provided.
    NoKey,
    /// Daily budget is exhausted.
    OverDaily,
    /// Session budget is exhausted.
    OverSession,
}
/// Reasons no model judgment is available.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NoDecisionReason {
    /// Requests are disabled by configuration.
    Disabled,
    /// State exceeds the configured UTF-8 byte limit.
    StateTooLarge,
    /// Every available model failed, or no model is available.
    AllFailed,
    /// The API rejected a request another model cannot fix.
    RequestRejected,
    /// The model chain contains an unpinned alias.
    InvalidConfiguration,
}
/// Fail-open result; network and validation errors never escape as `Result`.
#[derive(Clone, Debug, PartialEq)]
pub enum Outcome {
    /// A validated model decision.
    Decision(Decision),
    /// No judgment; let the caller's existing behavior continue.
    NoDecision(NoDecisionReason),
    /// Use the caller's deterministic implementation.
    CodeOnly(CodeOnlyReason),
}
/// Decisions client with injected transport, clock, and spend ledger.
#[allow(clippy::module_name_repetitions)] // Public names follow the decision-client contract.
pub struct DecisionClient<T: Transport, C: Clock, L: SpendLedger> {
    config: DecideConfig,
    key: Option<ApiKey>,
    transport: T,
    clock: Arc<C>,
    ledger: L,
    chain: ModelChain<Arc<C>>,
    cache: DecisionCache<Arc<C>>,
    budget: Budget,
}
impl<T: Transport, C: Clock, L: SpendLedger> DecisionClient<T, C, L> {
    /// Create an isolated client with an empty cache and fresh health state.
    #[must_use]
    pub fn new(
        config: DecideConfig,
        key: Option<ApiKey>,
        transport: T,
        clock: C,
        ledger: L,
    ) -> Self {
        let clock = Arc::new(clock);
        let chain = ModelChain::new(&config.chain, clock.clone());
        let cache = DecisionCache::new(clock.clone());
        let budget = Budget::new(config.daily_cap_usd, config.session_cap_usd);
        Self {
            config,
            key,
            transport,
            clock,
            ledger,
            chain,
            cache,
            budget,
        }
    }
    /// Ask one model at a time until a valid answer is available.
    ///
    /// # Panics
    /// Requires a Tokio runtime with its time driver enabled.
    #[allow(clippy::needless_pass_by_value)] // The async operation owns its request until completion.
    pub async fn decide(&self, req: DecideRequest) -> Outcome {
        if let Some(outcome) = self.gate(&req) {
            return outcome;
        }
        let models = self.chain.active_models();
        for model in &models {
            if let Some(cached) = self.cached(model, &req) {
                return cached;
            }
        }
        for model in &models {
            let outcome = self.attempt(model, &req).await;
            if !matches!(outcome, Outcome::NoDecision(NoDecisionReason::AllFailed)) {
                return outcome;
            }
        }
        Outcome::NoDecision(NoDecisionReason::AllFailed)
    }
    /// Ask the first two active models concurrently, returning independent outcomes.
    /// Each model is attempted once; combining their answers belongs to calibration.
    ///
    /// # Panics
    /// Requires a Tokio runtime with its time driver enabled.
    #[allow(clippy::needless_pass_by_value)] // The async operation owns its request until completion.
    pub async fn decide_agreement(&self, req: DecideRequest) -> (Outcome, Outcome) {
        if let Some(outcome) = self.gate(&req) {
            return (outcome.clone(), outcome);
        }
        let models = self.chain.active_models();
        tokio::join!(
            self.agreement_slot(models.first().map(String::as_str), &req),
            self.agreement_slot(models.get(1).map(String::as_str), &req)
        )
    }
    /// Probe each free slot explicitly and activate only successful slots.
    /// Probes bypass the cache but obey the ordinary key, size, budget, and timeout checks.
    ///
    /// # Panics
    /// Requires a Tokio runtime with its time driver enabled.
    #[allow(clippy::needless_pass_by_value)] // The async operation owns its request until completion.
    pub async fn reprobe_free(&self, req: DecideRequest) -> Vec<Outcome> {
        if let Some(outcome) = self.gate(&req) {
            return vec![outcome];
        }
        let mut outcomes = Vec::new();
        for model in self.chain.free_models() {
            let outcome = self.attempt(&model, &req).await;
            self.chain
                .reprobe_free(&model, matches!(outcome, Outcome::Decision(_)));
            let stop = matches!(
                outcome,
                Outcome::CodeOnly(_) | Outcome::NoDecision(NoDecisionReason::RequestRejected)
            );
            outcomes.push(outcome);
            if stop {
                break;
            }
        }
        outcomes
    }
    fn gate(&self, req: &DecideRequest) -> Option<Outcome> {
        if !self.config.enabled {
            return Some(Outcome::NoDecision(NoDecisionReason::Disabled));
        }
        if self.key.is_none() {
            return Some(Outcome::CodeOnly(CodeOnlyReason::NoKey));
        }
        if let Some(outcome) = self.budget_gate(req) {
            return Some(outcome);
        }
        if req.state.len() > self.config.max_state_bytes {
            return Some(Outcome::NoDecision(NoDecisionReason::StateTooLarge));
        }
        if self.config.validate().is_err() {
            return Some(Outcome::NoDecision(NoDecisionReason::InvalidConfiguration));
        }
        None
    }
    fn budget_gate(&self, req: &DecideRequest) -> Option<Outcome> {
        match self
            .budget
            .check(&self.ledger, self.clock.now_ms(), &req.session_id)
        {
            BudgetVerdict::Allowed => None,
            BudgetVerdict::OverDaily => Some(Outcome::CodeOnly(CodeOnlyReason::OverDaily)),
            BudgetVerdict::OverSession => Some(Outcome::CodeOnly(CodeOnlyReason::OverSession)),
        }
    }
    fn cached(&self, model: &str, req: &DecideRequest) -> Option<Outcome> {
        let response = self
            .cache
            .get(model, &req.state, &req.questions, req.purpose)?;
        self.record(req, model, Some(&response), 0, "ok", true);
        Some(decision(model, response, 0, true))
    }
    async fn agreement_slot(&self, model: Option<&str>, req: &DecideRequest) -> Outcome {
        let Some(model) = model else {
            return Outcome::NoDecision(NoDecisionReason::AllFailed);
        };
        if let Some(cached) = self.cached(model, req) {
            return cached;
        }
        self.attempt(model, req).await
    }
    async fn send(
        &self,
        model: &str,
        req: &DecideRequest,
        key: &ApiKey,
    ) -> Result<HttpReply, Failure> {
        let request = DecisionsRequest {
            model: model.into(),
            state: serde_json::Value::String(req.state.clone()),
            questions: req.questions.clone(),
            provider: Some(self.config.provider.clone()),
            session_id: Some(req.session_id.clone()),
            user: None,
        };
        let body = serde_json::to_string(&request).map_err(|_| Failure::Serialization)?;
        match tokio::time::timeout(
            Duration::from_millis(self.config.timeout_ms),
            self.transport.post_decision(key, body),
        )
        .await
        {
            Ok(Ok(reply)) => Ok(reply),
            Ok(Err(_)) => Err(Failure::Transport),
            Err(_) => Err(Failure::Timeout),
        }
    }
    async fn attempt(&self, model: &str, req: &DecideRequest) -> Outcome {
        // A failed request may still have accrued spend; check again before each retry.
        if let Some(outcome) = self.budget_gate(req) {
            return outcome;
        }
        let Some(key) = self.key.as_ref() else {
            return Outcome::CodeOnly(CodeOnlyReason::NoKey);
        };
        let started = tokio::time::Instant::now();
        let reply = self.send(model, req, key).await;
        let latency_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
        let reply = match reply {
            Ok(reply) => reply,
            Err(failure) => return self.failed(model, req, None, latency_ms, failure),
        };
        if !(200..300).contains(&reply.status) {
            return self.failed(model, req, None, latency_ms, Failure::Http(reply.status));
        }
        let Ok(response) = serde_json::from_str::<DecisionsResponse>(&reply.body) else {
            return self.failed(model, req, None, latency_ms, Failure::Unparsable);
        };
        if api::validate(&req.questions, &response).is_err() {
            return self.failed(
                model,
                req,
                Some(&response),
                latency_ms,
                Failure::InvalidAnswer,
            );
        }
        self.chain.record_success(model);
        self.record(req, model, Some(&response), latency_ms, "ok", false);
        self.cache.insert(
            model,
            &req.state,
            &req.questions,
            req.purpose,
            response.clone(),
        );
        decision(model, response, latency_ms, false)
    }
    fn failed(
        &self,
        model: &str,
        req: &DecideRequest,
        response: Option<&DecisionsResponse>,
        latency_ms: u64,
        failure: Failure,
    ) -> Outcome {
        let reason = failure.reason();
        self.record(req, model, response, latency_ms, &reason, false);
        tracing::warn!(
            model,
            purpose = req.purpose.as_str(),
            reason,
            "decision attempt failed"
        );
        if matches!(failure, Failure::Http(status) if is_non_retryable(status)) {
            Outcome::NoDecision(NoDecisionReason::RequestRejected)
        } else {
            self.chain.record_failure(model);
            Outcome::NoDecision(NoDecisionReason::AllFailed)
        }
    }
    fn record(
        &self,
        req: &DecideRequest,
        model: &str,
        response: Option<&DecisionsResponse>,
        latency_ms: u64,
        outcome: &str,
        cached: bool,
    ) {
        self.ledger.record(DecisionRow {
            ts_ms: self.clock.now_ms(),
            session_id: req.session_id.clone(),
            purpose: req.purpose,
            requested_model: model.into(),
            model_version: response.map(|r| r.model.clone()),
            provider: response.and_then(|r| r.provider.clone()),
            input_tokens: response.map_or(0, |r| r.usage.input_tokens),
            output_tokens: response.map_or(0, |r| r.usage.output_tokens),
            cost_usd: if cached {
                Some(0.0)
            } else {
                response.and_then(|r| r.usage.cost)
            },
            latency_ms,
            question_names: req.questions.keys().cloned().collect(),
            outcome: outcome.into(),
            cached,
        });
    }
}
#[derive(Clone, Copy)]
enum Failure {
    Serialization,
    Transport,
    Timeout,
    Http(u16),
    Unparsable,
    InvalidAnswer,
}
impl Failure {
    fn reason(self) -> String {
        match self {
            Self::Serialization => "serialization".into(),
            Self::Transport => "transport".into(),
            Self::Timeout => "timeout".into(),
            Self::Http(status) => format!("http_{status}"),
            Self::Unparsable => "unparsable_body".into(),
            Self::InvalidAnswer => "invalid_answer".into(),
        }
    }
}
fn decision(model: &str, response: DecisionsResponse, latency_ms: u64, cached: bool) -> Outcome {
    Outcome::Decision(Decision {
        model_version: response.model,
        requested_model: model.into(),
        provider: response.provider,
        answers: response.answers,
        usage: response.usage,
        latency_ms,
        cached,
    })
}
