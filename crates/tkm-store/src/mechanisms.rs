//! Typed persistence for facts, tool evidence, checkout deny lists, and envelopes.
use crate::{Store, StoreError};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tkm_core::model::{Host, RepoKey, Timestamp};

/// Maximum number of good-turn samples retained per repository and host.
pub const ENVELOPE_RING: usize = 500;

/// Lifecycle state of a repository fact.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FactState {
    /// Awaiting confirmation.
    Candidate,
    /// Available for use.
    Active,
    /// No longer in use.
    Retired,
}
impl FactState {
    /// Returns the persisted representation.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Candidate => "candidate",
            Self::Active => "active",
            Self::Retired => "retired",
        }
    }
}
impl std::str::FromStr for FactState {
    type Err = StoreError;
    /// Parses a persisted lifecycle state.
    ///
    /// # Errors
    /// Returns an error for unknown states.
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "candidate" => Ok(Self::Candidate),
            "active" => Ok(Self::Active),
            "retired" => Ok(Self::Retired),
            _ => Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("unknown fact state: {value}"),
            )
            .into()),
        }
    }
}

/// FactRecord persisted record.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FactRecord {
    /// Repository identity.
    pub repo_key: RepoKey,
    /// Fact identifier.
    pub fact_id: String,
    /// Extraction family.
    pub family: String,
    /// Fact scope.
    pub scope: String,
    /// Template parameters.
    pub text_params: serde_json::Value,
    /// Lifecycle state.
    pub state: FactState,
    /// Lifecycle score.
    pub score: f64,
    /// Confirmation count.
    pub confirmations: u32,
    /// Contradiction count.
    pub contradictions: u32,
    /// Firing count.
    pub fire_count: u32,
    /// Observed sessions.
    pub fire_sessions: u32,
}

/// ToolEvidence persisted record.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolEvidence {
    /// Repository identity.
    pub repo_key: RepoKey,
    /// Harness identity.
    pub host: Host,
    /// Tool name.
    pub tool: String,
    /// Consecutive unused prompts.
    pub prompts_unused: u32,
    /// Consecutive unused sessions.
    pub sessions_unused: u32,
    /// Most recent use.
    pub last_used: Option<Timestamp>,
}

/// Deny-list entry scoped to a checkout, not a repository.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DenylistEntry {
    /// Checkout path.
    pub checkout_path: PathBuf,
    /// Harness identity.
    pub host: Host,
    /// Tool name.
    pub tool: String,
    /// Write timestamp.
    pub written_at: Timestamp,
    /// Whether the entry was restored.
    pub restored: bool,
}

impl Store {
    /// Insert candidate fact.
    ///
    /// # Errors
    /// Returns an error if the operation fails.
    pub fn insert_candidate_fact(&self, fact: &FactRecord) -> Result<(), StoreError> {
        unimplemented!()
    }
    /// Get fact.
    ///
    /// # Errors
    /// Returns an error if the operation fails.
    pub fn get_fact(&self, repo: &RepoKey, id: &str) -> Result<Option<FactRecord>, StoreError> {
        unimplemented!()
    }
    /// List facts.
    ///
    /// # Errors
    /// Returns an error if the operation fails.
    pub fn list_facts(
        &self,
        repo: &RepoKey,
        state: Option<FactState>,
    ) -> Result<Vec<FactRecord>, StoreError> {
        unimplemented!()
    }
    /// Update fact stats.
    ///
    /// # Errors
    /// Returns an error if the operation fails.
    #[expect(
        clippy::too_many_arguments,
        reason = "The public API accepts all fact statistics explicitly."
    )]
    pub fn update_fact_stats(
        &self,
        repo: &RepoKey,
        id: &str,
        score: f64,
        confirmations: u32,
        contradictions: u32,
        fire_count: u32,
        fire_sessions: u32,
    ) -> Result<(), StoreError> {
        unimplemented!()
    }
    /// Transition fact.
    ///
    /// # Errors
    /// Returns an error if the operation fails.
    pub fn transition_fact(
        &self,
        repo: &RepoKey,
        id: &str,
        to: FactState,
    ) -> Result<(), StoreError> {
        unimplemented!()
    }
    /// Record tool used.
    ///
    /// # Errors
    /// Returns an error if the operation fails.
    pub fn record_tool_used(
        &self,
        repo: &RepoKey,
        host: Host,
        tool: &str,
        at: Timestamp,
    ) -> Result<(), StoreError> {
        unimplemented!()
    }
    /// Record tool unused prompt.
    ///
    /// # Errors
    /// Returns an error if the operation fails.
    pub fn record_tool_unused_prompt(
        &self,
        repo: &RepoKey,
        host: Host,
        tool: &str,
    ) -> Result<(), StoreError> {
        unimplemented!()
    }
    /// Record tool unused session.
    ///
    /// # Errors
    /// Returns an error if the operation fails.
    pub fn record_tool_unused_session(
        &self,
        repo: &RepoKey,
        host: Host,
        tool: &str,
    ) -> Result<(), StoreError> {
        unimplemented!()
    }
    /// Get tool evidence.
    ///
    /// # Errors
    /// Returns an error if the operation fails.
    pub fn get_tool_evidence(
        &self,
        repo: &RepoKey,
        host: Host,
        tool: &str,
    ) -> Result<Option<ToolEvidence>, StoreError> {
        unimplemented!()
    }
    /// List tool evidence.
    ///
    /// # Errors
    /// Returns an error if the operation fails.
    pub fn list_tool_evidence(
        &self,
        repo: &RepoKey,
        host: Host,
    ) -> Result<Vec<ToolEvidence>, StoreError> {
        unimplemented!()
    }
    /// Record denylist write.
    ///
    /// # Errors
    /// Returns an error if the operation fails.
    pub fn record_denylist_write(&self, entry: &DenylistEntry) -> Result<(), StoreError> {
        unimplemented!()
    }
    /// Mark denylist restored.
    ///
    /// # Errors
    /// Returns an error if the operation fails.
    pub fn mark_denylist_restored(
        &self,
        checkout: &Path,
        host: Host,
        tool: &str,
    ) -> Result<(), StoreError> {
        unimplemented!()
    }
    /// List denylist.
    ///
    /// # Errors
    /// Returns an error if the operation fails.
    pub fn list_denylist(&self, checkout: &Path) -> Result<Vec<DenylistEntry>, StoreError> {
        unimplemented!()
    }
    /// Push envelope sample.
    ///
    /// # Errors
    /// Returns an error if the operation fails.
    pub fn push_envelope_sample(
        &self,
        repo: &RepoKey,
        host: Host,
        calls: u32,
    ) -> Result<(), StoreError> {
        unimplemented!()
    }
    /// Envelope samples.
    ///
    /// # Errors
    /// Returns an error if the operation fails.
    pub fn envelope_samples(&self, repo: &RepoKey, host: Host) -> Result<Vec<u32>, StoreError> {
        unimplemented!()
    }
}
