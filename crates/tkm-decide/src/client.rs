//! Fail-open, budgeted Decisions client and injectable HTTP transport.
use crate::{
    api::{Answer, Question, Usage},
    budget::SpendLedger,
    cache::Purpose,
    chain::Clock,
    config::DecideConfig,
    key::ApiKey,
};
use std::{collections::BTreeMap, future::Future};
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
    async fn post_decision(
        &self,
        _key: &ApiKey,
        _body: String,
    ) -> Result<HttpReply, TransportError> {
        let _ = &self.client;
        Err(TransportError::Request)
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
pub struct DecisionClient<T: Transport, C: Clock, L: SpendLedger> {
    config: DecideConfig,
    key: Option<ApiKey>,
    transport: T,
    clock: C,
    ledger: L,
}
impl<T: Transport, C: Clock, L: SpendLedger> DecisionClient<T, C, L> {
    /// Create an isolated client with an empty cache and fresh health state.
    #[must_use]
    pub const fn new(
        config: DecideConfig,
        key: Option<ApiKey>,
        transport: T,
        clock: C,
        ledger: L,
    ) -> Self {
        Self {
            config,
            key,
            transport,
            clock,
            ledger,
        }
    }
    /// Ask one model at a time until a valid answer is available.
    #[allow(clippy::unused_async)] // Public async contract; the implementation follows in the next commit.
    pub async fn decide(&self, _req: DecideRequest) -> Outcome {
        let _ = (
            &self.config,
            &self.key,
            &self.transport,
            &self.clock,
            &self.ledger,
        );
        Outcome::NoDecision(NoDecisionReason::AllFailed)
    }
    /// Ask the first two active models concurrently, returning independent outcomes.
    #[allow(clippy::unused_async)] // Public async contract; the implementation follows in the next commit.
    pub async fn decide_agreement(&self, _req: DecideRequest) -> (Outcome, Outcome) {
        (
            Outcome::NoDecision(NoDecisionReason::AllFailed),
            Outcome::NoDecision(NoDecisionReason::AllFailed),
        )
    }
    /// Probe each free slot explicitly and activate only successful slots.
    #[allow(clippy::unused_async)] // Public async contract; the implementation follows in the next commit.
    pub async fn reprobe_free(&self, _req: DecideRequest) -> Vec<Outcome> {
        Vec::new()
    }
}
