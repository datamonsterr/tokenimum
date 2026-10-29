use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;

/// Failure without any transcript or prompt content in its display text.
#[derive(Debug, thiserror::Error)]
pub enum BenchError {
    /// Filesystem failure.
    #[error("benchmark I/O: {0}")]
    Io(#[from] std::io::Error),
    /// Malformed JSON; the input text is never included.
    #[error("benchmark JSON: {0}")]
    Json(#[from] serde_json::Error),
    /// Invalid benchmark metadata or an unmatched configuration.
    #[error("benchmark: {0}")]
    Invalid(String),
    /// Existing host reader failure.
    #[error("benchmark host reader: {0}")]
    Host(#[from] tkm_hosts::AdapterError),
}

pub(super) fn invalid(message: &str) -> BenchError {
    BenchError::Invalid(message.to_owned())
}

/// Four independently billed token classes; cache tokens are never subtracted from input.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Tokens {
    /// Uncached input tokens.
    pub input: u64,
    /// Cache creation input tokens.
    pub cache_creation: u64,
    /// Cache read input tokens.
    pub cache_read: u64,
    /// Output tokens.
    pub output: u64,
}
impl Tokens {
    pub(super) fn add(&mut self, other: Self) {
        self.input = self.input.saturating_add(other.input);
        self.cache_creation = self.cache_creation.saturating_add(other.cache_creation);
        self.cache_read = self.cache_read.saturating_add(other.cache_read);
        self.output = self.output.saturating_add(other.output);
    }
    pub(super) fn max(self, other: Self) -> Self {
        Self {
            input: self.input.max(other.input),
            cache_creation: self.cache_creation.max(other.cache_creation),
            cache_read: self.cache_read.max(other.cache_read),
            output: self.output.max(other.output),
        }
    }
    pub(super) fn size(self) -> u64 {
        self.input
            .saturating_add(self.cache_creation)
            .saturating_add(self.cache_read)
            .saturating_add(self.output)
    }
}

/// Historical USD prices per million tokens, supplied with the private bundle.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct ModelPrice {
    /// Uncached input price.
    pub input: f64,
    /// Cache creation price.
    pub cache_creation: f64,
    /// Cache read price.
    pub cache_read: f64,
    /// Output price.
    pub output: f64,
}
impl ModelPrice {
    /// Price usage, rejecting absent, negative or non-finite price data.
    ///
    /// # Errors
    /// Returns an error for invalid rates.
    #[allow(clippy::cast_precision_loss)] // Token billing is an estimate expressed in USD.
    pub fn cost(self, tokens: Tokens) -> Result<f64, BenchError> {
        let rates = [
            self.input,
            self.cache_creation,
            self.cache_read,
            self.output,
        ];
        if rates.iter().any(|v| !v.is_finite() || *v < 0.0) {
            return Err(invalid("invalid model price"));
        }
        let cost = (tokens.input as f64 * self.input
            + tokens.cache_creation as f64 * self.cache_creation
            + tokens.cache_read as f64 * self.cache_read
            + tokens.output as f64 * self.output)
            / 1_000_000.0;
        if !cost.is_finite() {
            return Err(invalid("model price overflow"));
        }
        Ok(cost)
    }
}

/// A private manifest entry, never copied into the public report.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct SessionSpec {
    /// Stable corpus-local session identifier.
    pub id: String,
    /// JSONL path relative to the bundle (or absolute for library callers).
    pub path: PathBuf,
    /// Explicit spawning session, required for every agent session.
    #[serde(default)]
    pub parent: Option<String>,
    /// Branch-specific `git log --format=%H%x09%ct` file.
    #[serde(default)]
    pub git_log: PathBuf,
    /// Optional verified HEAD override, captured from a reflog or original worktree.
    #[serde(default)]
    pub start_commit: Option<String>,
    /// Original end commit; required for top-level replay outcome checks.
    #[serde(default)]
    pub end_commit: Option<String>,
}

/// Private bundle metadata.
#[derive(Debug, Serialize, Deserialize)]
pub struct Manifest {
    /// One pinned Claude Code release shared by both arms.
    pub claude_version: String,
    /// Matched effort setting.
    pub effort: String,
    /// Matched permission mode.
    pub permission_mode: String,
    /// CI-equivalent commands, as argument vectors without shell interpretation.
    pub tests: Vec<Vec<String>>,
    /// Historical pricing, keyed by exact recorded model identifier.
    pub prices: BTreeMap<String, ModelPrice>,
    /// All transcripts, including explicit agent parent links.
    pub sessions: Vec<SessionSpec>,
}

/// A commit reachable from the session's historical branch.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Commit {
    /// Full object identifier.
    pub hash: String,
    /// Committer timestamp in Unix milliseconds.
    pub timestamp_ms: i64,
}

/// Last commit at or before a session's first timestamp; `None` means empty repository.
#[must_use]
pub fn anchor(commits: &[Commit], first_timestamp_ms: i64) -> Option<String> {
    let mut best: Option<&Commit> = None;
    for commit in commits {
        if commit.timestamp_ms <= first_timestamp_ms
            && best.is_none_or(|current| commit.timestamp_ms > current.timestamp_ms)
        {
            best = Some(commit);
        }
    }
    best.map(|commit| commit.hash.clone())
}

/// Parse branch-specific git log output without author-date heuristics.
///
/// # Errors
/// Rejects malformed hashes or timestamps.
pub fn parse_git_log(text: &str) -> Result<Vec<Commit>, BenchError> {
    text.lines()
        .filter(|line| !line.is_empty())
        .map(|line| {
            let (hash, timestamp) = line
                .split_once('\t')
                .ok_or_else(|| invalid("invalid git log line"))?;
            validate_commit(hash)?;
            let timestamp_ms = timestamp
                .parse::<i64>()
                .ok()
                .and_then(|v| v.checked_mul(1000))
                .ok_or_else(|| invalid("invalid commit timestamp"))?;
            Ok(Commit {
                hash: hash.into(),
                timestamp_ms,
            })
        })
        .collect()
}
pub(super) fn validate_commit(hash: &str) -> Result<(), BenchError> {
    if !matches!(hash.len(), 40 | 64) || !hash.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(invalid("commit must be a full object identifier"));
    }
    Ok(())
}

/// A real typed prompt and the model of its following assistant message.
#[derive(Debug, Clone, Serialize)]
pub struct Prompt {
    /// Private text, written only into a temporary replay plan.
    pub text: String,
    /// Exact recorded model; missing values prevent live execution.
    pub model: Option<String>,
}

/// Prompt-free session aggregates, including attributed agents.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct SessionMetrics {
    /// Tokens across all models.
    pub tokens: Tokens,
    /// Tokens by exact model identifier.
    pub by_model: BTreeMap<String, Tokens>,
    /// Typed or delegated turns.
    pub turns: u64,
    /// Tool invocations.
    pub tool_calls: u64,
    /// Failed tool invocations.
    pub tool_errors: u64,
    /// Session wall time; agents do not add overlapping durations.
    pub wall_ms: u64,
    /// Model-weighted usage cost.
    pub cost_usd: f64,
}

/// Baseline or measured arm; safe to store next to the private run.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct Baseline {
    /// Top-level session aggregates, keyed by private corpus identifiers.
    pub sessions: BTreeMap<String, SessionMetrics>,
    /// Model totals.
    pub by_model: BTreeMap<String, Tokens>,
    /// Total tokens.
    pub total: Tokens,
    /// Historical model-weighted total.
    pub cost_usd: f64,
    /// Skipped transcript identifiers and content-free reasons.
    pub skipped: BTreeMap<String, String>,
}
