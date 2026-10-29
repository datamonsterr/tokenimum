//! Typed session, turn, event, and usage ledger records and queries.
//!
//! Events preserve the evidence and templates used by `tkm why`.
use crate::{Store, StoreError};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tkm_core::model::{Host, RepoKey, Timestamp, Usage};

/// Host-scoped session identity.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionKey {
    /// Harness identity.
    pub host: Host,
    /// Harness home path.
    pub agent_home: String,
    /// Host session identifier.
    pub session_id: String,
}
/// Session data for insertion or update.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NewSession {
    /// Host-scoped identity.
    pub key: SessionKey,
    /// Repository identity.
    pub repo_key: RepoKey,
    /// Working directory.
    pub cwd: PathBuf,
    /// Start time.
    pub started: Timestamp,
    /// End time if completed.
    pub ended: Option<Timestamp>,
    /// Fold processing state.
    pub fold_status: String,
    /// Whether actions were shadowed.
    pub shadow: bool,
    /// Integration level.
    pub integration_level: String,
}
/// Persisted session data.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionRecord {
    /// Local session identifier.
    pub id: i64,
    /// Host-scoped identity.
    pub key: SessionKey,
    /// Repository identity.
    pub repo_key: RepoKey,
    /// Working directory.
    pub cwd: PathBuf,
    /// Start time.
    pub started: Timestamp,
    /// End time if completed.
    pub ended: Option<Timestamp>,
    /// Fold processing state.
    pub fold_status: String,
    /// Whether actions were shadowed.
    pub shadow: bool,
    /// Integration level.
    pub integration_level: String,
}
/// A session turn.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TurnRecord {
    /// Local session identifier.
    pub session_id: i64,
    /// Position within the session.
    pub turn_idx: i64,
    /// Tool call count.
    pub calls: i64,
    /// Whether the turn ended well.
    pub ended_well: bool,
    /// Whether the turn escalated.
    pub escalated: bool,
    /// Optional digest hash.
    pub digest_hash: Option<String>,
}
/// Action data for insertion.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NewEvent {
    /// Action kind.
    pub kind: String,
    /// Local session identifier.
    pub session_id: i64,
    /// Action inputs.
    pub inputs: serde_json::Value,
    /// Evidence identifiers.
    pub evidence_ids: Vec<i64>,
    /// Explanation template identifier.
    pub explanation_template_id: String,
    /// Explanation parameters.
    pub params: serde_json::Value,
}
/// Persisted action, the source for `tkm why` explanations.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EventRecord {
    /// Local event identifier.
    pub id: i64,
    /// Action kind.
    pub kind: String,
    /// Local session identifier.
    pub session_id: i64,
    /// Action inputs.
    pub inputs: serde_json::Value,
    /// Evidence identifiers.
    pub evidence_ids: Vec<i64>,
    /// Explanation template identifier.
    pub explanation_template_id: String,
    /// Explanation parameters.
    pub params: serde_json::Value,
}
/// Session usage and cost accounting.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LedgerRecord {
    /// Local session identifier.
    pub session_id: i64,
    /// Usage by token class.
    pub usage: Usage,
    /// Priced session cost.
    pub priced_cost: f64,
    /// Decision overhead tokens.
    pub overhead_tokens: u64,
    /// Decision overhead cost.
    pub overhead_cost: f64,
}

impl Store {
    /// Upsert session.
    ///
    /// # Errors
    /// Returns an error if the operation fails.
    pub fn upsert_session(&self, session: &NewSession) -> Result<i64, StoreError> {
        unimplemented!()
    }
    /// Get session.
    ///
    /// # Errors
    /// Returns an error if the operation fails.
    pub fn get_session(&self, id: i64) -> Result<Option<SessionRecord>, StoreError> {
        unimplemented!()
    }
    /// Find session.
    ///
    /// # Errors
    /// Returns an error if the operation fails.
    pub fn find_session(&self, key: &SessionKey) -> Result<Option<SessionRecord>, StoreError> {
        unimplemented!()
    }
    /// End session.
    ///
    /// # Errors
    /// Returns an error if the operation fails.
    pub fn end_session(&self, id: i64, ended: Timestamp) -> Result<(), StoreError> {
        unimplemented!()
    }
    /// Set session fold status.
    ///
    /// # Errors
    /// Returns an error if the operation fails.
    pub fn set_session_fold_status(&self, id: i64, status: &str) -> Result<(), StoreError> {
        unimplemented!()
    }
    /// Set session shadow.
    ///
    /// # Errors
    /// Returns an error if the operation fails.
    pub fn set_session_shadow(&self, id: i64, shadow: bool) -> Result<(), StoreError> {
        unimplemented!()
    }
    /// Set integration level.
    ///
    /// # Errors
    /// Returns an error if the operation fails.
    pub fn set_integration_level(&self, id: i64, level: &str) -> Result<(), StoreError> {
        unimplemented!()
    }
    /// List sessions by repo.
    ///
    /// # Errors
    /// Returns an error if the operation fails.
    pub fn list_sessions_by_repo(
        &self,
        key: &RepoKey,
        limit: i64,
    ) -> Result<Vec<SessionRecord>, StoreError> {
        unimplemented!()
    }
    /// Upsert turn.
    ///
    /// # Errors
    /// Returns an error if the operation fails.
    pub fn upsert_turn(&self, turn: &TurnRecord) -> Result<(), StoreError> {
        unimplemented!()
    }
    /// List turns.
    ///
    /// # Errors
    /// Returns an error if the operation fails.
    pub fn list_turns(&self, session_id: i64) -> Result<Vec<TurnRecord>, StoreError> {
        unimplemented!()
    }
    /// Insert event.
    ///
    /// # Errors
    /// Returns an error if the operation fails.
    pub fn insert_event(&self, event: &NewEvent) -> Result<i64, StoreError> {
        unimplemented!()
    }
    /// Get event.
    ///
    /// # Errors
    /// Returns an error if the operation fails.
    pub fn get_event(&self, id: i64) -> Result<Option<EventRecord>, StoreError> {
        unimplemented!()
    }
    /// List events by session.
    ///
    /// # Errors
    /// Returns an error if the operation fails.
    pub fn list_events_by_session(&self, session_id: i64) -> Result<Vec<EventRecord>, StoreError> {
        unimplemented!()
    }
    /// List recent events.
    ///
    /// # Errors
    /// Returns an error if the operation fails.
    pub fn list_recent_events(&self, limit: i64) -> Result<Vec<EventRecord>, StoreError> {
        unimplemented!()
    }
    /// Upsert ledger.
    ///
    /// # Errors
    /// Returns an error if the operation fails.
    pub fn upsert_ledger(&self, ledger: &LedgerRecord) -> Result<(), StoreError> {
        unimplemented!()
    }
    /// Get ledger.
    ///
    /// # Errors
    /// Returns an error if the operation fails.
    pub fn get_ledger(&self, session_id: i64) -> Result<Option<LedgerRecord>, StoreError> {
        unimplemented!()
    }
}
