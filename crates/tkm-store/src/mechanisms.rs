//! Typed persistence for facts, tool evidence, checkout deny lists, and envelopes.
use crate::{Store, StoreError};
use rusqlite::{OptionalExtension, Transaction, TransactionBehavior, params};
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

/// Persisted repository fact and lifecycle statistics.
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

/// Persisted tool usage evidence.
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

fn text_error<E: std::error::Error + Send + Sync + 'static>(
    index: usize,
    error: E,
) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(index, rusqlite::types::Type::Text, Box::new(error))
}
fn read_fact(row: &rusqlite::Row<'_>) -> rusqlite::Result<FactRecord> {
    let json: String = row.get(4)?;
    let state: String = row.get(5)?;
    Ok(FactRecord {
        repo_key: RepoKey(row.get(0)?),
        fact_id: row.get(1)?,
        family: row.get(2)?,
        scope: row.get(3)?,
        text_params: serde_json::from_str(&json).map_err(|e| text_error(4, e))?,
        state: state.parse().map_err(|e| text_error(5, e))?,
        score: row.get(6)?,
        confirmations: row.get(7)?,
        contradictions: row.get(8)?,
        fire_count: row.get(9)?,
        fire_sessions: row.get(10)?,
    })
}
fn read_tool(row: &rusqlite::Row<'_>) -> rusqlite::Result<ToolEvidence> {
    let host: String = row.get(1)?;
    Ok(ToolEvidence {
        repo_key: RepoKey(row.get(0)?),
        host: host.parse().map_err(|e| text_error(1, e))?,
        tool: row.get(2)?,
        prompts_unused: row.get(3)?,
        sessions_unused: row.get(4)?,
        last_used: row.get::<_, Option<i64>>(5)?.map(Timestamp),
    })
}
fn read_denylist(row: &rusqlite::Row<'_>) -> rusqlite::Result<DenylistEntry> {
    let host: String = row.get(1)?;
    Ok(DenylistEntry {
        checkout_path: PathBuf::from(row.get::<_, String>(0)?),
        host: host.parse().map_err(|e| text_error(1, e))?,
        tool: row.get(2)?,
        written_at: Timestamp(row.get(3)?),
        restored: row.get(4)?,
    })
}
fn path_text(path: &Path) -> Result<&str, StoreError> {
    path.to_str()
        .ok_or_else(|| rusqlite::Error::InvalidPath(path.to_path_buf()).into())
}
fn require_changed(count: usize) -> Result<(), StoreError> {
    if count == 0 {
        return Err(rusqlite::Error::QueryReturnedNoRows.into());
    }
    Ok(())
}

impl Store {
    /// Insert a fact as a candidate, regardless of its supplied state.
    ///
    /// # Errors
    /// Returns an error for a duplicate fact, invalid JSON, or database failure.
    pub fn insert_candidate_fact(&self, fact: &FactRecord) -> Result<(), StoreError> {
        self.conn.execute("INSERT INTO facts (repo_key, fact_id, family, scope, text_params, state, score, confirmations, contradictions, fire_count, fire_sessions) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)", params![fact.repo_key.0, fact.fact_id, fact.family, fact.scope, serde_json::to_string(&fact.text_params)?, FactState::Candidate.as_str(), fact.score, fact.confirmations, fact.contradictions, fact.fire_count, fact.fire_sessions])?;
        Ok(())
    }
    /// Get fact.
    ///
    /// # Errors
    /// Returns an error if the operation fails.
    pub fn get_fact(&self, repo: &RepoKey, id: &str) -> Result<Option<FactRecord>, StoreError> {
        Ok(self.conn.query_row("SELECT repo_key, fact_id, family, scope, text_params, state, score, confirmations, contradictions, fire_count, fire_sessions FROM facts WHERE repo_key = ?1 AND fact_id = ?2", params![repo.0, id], read_fact).optional()?)
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
        let mut stmt = self.conn.prepare("SELECT repo_key, fact_id, family, scope, text_params, state, score, confirmations, contradictions, fire_count, fire_sessions FROM facts WHERE repo_key = ?1 AND (?2 IS NULL OR state = ?2) ORDER BY fact_id")?;
        let rows = stmt.query_map(params![repo.0, state.map(FactState::as_str)], read_fact)?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
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
        require_changed(self.conn.execute("UPDATE facts SET score = ?3, confirmations = ?4, contradictions = ?5, fire_count = ?6, fire_sessions = ?7 WHERE repo_key = ?1 AND fact_id = ?2", params![repo.0, id, score, confirmations, contradictions, fire_count, fire_sessions])?)
    }
    /// Transition a fact in an IMMEDIATE transaction.
    ///
    /// This uses a shared connection reference; nested transactions fail at runtime.
    ///
    /// # Errors
    /// Returns an error if the operation fails.
    pub fn transition_fact(
        &self,
        repo: &RepoKey,
        id: &str,
        to: FactState,
    ) -> Result<(), StoreError> {
        let tx = Transaction::new_unchecked(&self.conn, TransactionBehavior::Immediate)?;
        let state: String = tx.query_row(
            "SELECT state FROM facts WHERE repo_key = ?1 AND fact_id = ?2",
            params![repo.0, id],
            |row| row.get(0),
        )?;
        let from: FactState = state.parse()?;
        if !matches!(
            (from, to),
            (FactState::Candidate, FactState::Active | FactState::Retired)
                | (FactState::Active, FactState::Retired)
                | (FactState::Retired, FactState::Candidate)
        ) {
            return Err(StoreError::InvalidTransition);
        }
        tx.execute(
            "UPDATE facts SET state = ?3 WHERE repo_key = ?1 AND fact_id = ?2",
            params![repo.0, id, to.as_str()],
        )?;
        tx.commit()?;
        Ok(())
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
        self.conn.execute("INSERT INTO tool_evidence (repo_key, host, tool, prompts_unused, sessions_unused, last_used) VALUES (?1, ?2, ?3, 0, 0, ?4) ON CONFLICT(repo_key, host, tool) DO UPDATE SET prompts_unused = 0, sessions_unused = 0, last_used = excluded.last_used", params![repo.0, host.as_str(), tool, at.0])?;
        Ok(())
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
        self.conn.execute("INSERT INTO tool_evidence (repo_key, host, tool, prompts_unused, sessions_unused, last_used) VALUES (?1, ?2, ?3, 1, 0, NULL) ON CONFLICT(repo_key, host, tool) DO UPDATE SET prompts_unused = prompts_unused + 1", params![repo.0, host.as_str(), tool])?;
        Ok(())
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
        self.conn.execute("INSERT INTO tool_evidence (repo_key, host, tool, prompts_unused, sessions_unused, last_used) VALUES (?1, ?2, ?3, 0, 1, NULL) ON CONFLICT(repo_key, host, tool) DO UPDATE SET sessions_unused = sessions_unused + 1", params![repo.0, host.as_str(), tool])?;
        Ok(())
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
        Ok(self.conn.query_row("SELECT repo_key, host, tool, prompts_unused, sessions_unused, last_used FROM tool_evidence WHERE repo_key = ?1 AND host = ?2 AND tool = ?3", params![repo.0, host.as_str(), tool], read_tool).optional()?)
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
        let mut stmt = self.conn.prepare("SELECT repo_key, host, tool, prompts_unused, sessions_unused, last_used FROM tool_evidence WHERE repo_key = ?1 AND host = ?2 ORDER BY tool")?;
        let rows = stmt.query_map(params![repo.0, host.as_str()], read_tool)?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }
    /// Record a deny-list entry scoped to its checkout path.
    ///
    /// # Errors
    /// Returns an error if the operation fails.
    pub fn record_denylist_write(&self, entry: &DenylistEntry) -> Result<(), StoreError> {
        self.conn.execute("INSERT INTO denylist_applied (checkout_path, host, tool, written_at, restored) VALUES (?1, ?2, ?3, ?4, ?5) ON CONFLICT(checkout_path, host, tool) DO UPDATE SET written_at = excluded.written_at, restored = excluded.restored", params![path_text(&entry.checkout_path)?, entry.host.as_str(), entry.tool, entry.written_at.0, entry.restored])?;
        Ok(())
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
        require_changed(self.conn.execute("UPDATE denylist_applied SET restored = 1 WHERE checkout_path = ?1 AND host = ?2 AND tool = ?3", params![path_text(checkout)?, host.as_str(), tool])?)
    }
    /// List deny-list entries for one checkout path.
    ///
    /// # Errors
    /// Returns an error if the operation fails.
    pub fn list_denylist(&self, checkout: &Path) -> Result<Vec<DenylistEntry>, StoreError> {
        let mut stmt = self.conn.prepare("SELECT checkout_path, host, tool, written_at, restored FROM denylist_applied WHERE checkout_path = ?1 ORDER BY host, tool")?;
        let rows = stmt.query_map([path_text(checkout)?], read_denylist)?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }
    /// Append a sample in an IMMEDIATE transaction, retaining the newest 500.
    ///
    /// This uses a shared connection reference; nested transactions fail at runtime.
    ///
    /// # Errors
    /// Returns an error if the operation fails.
    pub fn push_envelope_sample(
        &self,
        repo: &RepoKey,
        host: Host,
        calls: u32,
    ) -> Result<(), StoreError> {
        let tx = Transaction::new_unchecked(&self.conn, TransactionBehavior::Immediate)?;
        let json: Option<String> = tx
            .query_row(
                "SELECT samples FROM envelopes WHERE repo_key = ?1 AND host = ?2",
                params![repo.0, host.as_str()],
                |row| row.get(0),
            )
            .optional()?;
        let mut samples: Vec<u32> =
            json.map_or(Ok(Vec::new()), |json| serde_json::from_str(&json))?;
        samples.push(calls);
        if samples.len() > ENVELOPE_RING {
            samples.drain(..samples.len() - ENVELOPE_RING);
        }
        tx.execute("INSERT INTO envelopes (repo_key, host, samples) VALUES (?1, ?2, ?3) ON CONFLICT(repo_key, host) DO UPDATE SET samples = excluded.samples", params![repo.0, host.as_str(), serde_json::to_string(&samples)?])?;
        tx.commit()?;
        Ok(())
    }
    /// Envelope samples.
    ///
    /// # Errors
    /// Returns an error if the operation fails.
    pub fn envelope_samples(&self, repo: &RepoKey, host: Host) -> Result<Vec<u32>, StoreError> {
        let json: Option<String> = self
            .conn
            .query_row(
                "SELECT samples FROM envelopes WHERE repo_key = ?1 AND host = ?2",
                params![repo.0, host.as_str()],
                |row| row.get(0),
            )
            .optional()?;
        Ok(json.map_or(Ok(Vec::new()), |json| serde_json::from_str(&json))?)
    }
}
