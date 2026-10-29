//! Typed session, turn, event, and usage ledger records and queries.
//!
//! Events preserve the evidence and templates used by `tkm why`.
use crate::{Store, StoreError};
use rusqlite::{OptionalExtension, params, types::Type};
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
    /// Action time in Unix milliseconds, absent for historical writes.
    #[serde(default)]
    pub created_at: Option<Timestamp>,
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

const SESSION_COLUMNS: &str = "id, host, agent_home, session_id, repo_key, cwd, started_at, ended_at, fold_status, shadow, integration_level";
const EVENT_COLUMNS: &str =
    "id, kind, session_id, inputs, evidence_ids, explanation_template_id, params, created_at";

fn read_session(row: &rusqlite::Row<'_>) -> rusqlite::Result<SessionRecord> {
    let host: String = row.get(1)?;
    Ok(SessionRecord {
        id: row.get(0)?,
        key: SessionKey {
            host: host.parse::<Host>().map_err(|e| {
                rusqlite::Error::FromSqlConversionFailure(1, Type::Text, Box::new(e))
            })?,
            agent_home: row.get(2)?,
            session_id: row.get(3)?,
        },
        repo_key: RepoKey(row.get(4)?),
        cwd: PathBuf::from(row.get::<_, String>(5)?),
        started: Timestamp(row.get(6)?),
        ended: row.get::<_, Option<i64>>(7)?.map(Timestamp),
        fold_status: row.get(8)?,
        shadow: row.get(9)?,
        integration_level: row.get(10)?,
    })
}
fn read_turn(row: &rusqlite::Row<'_>) -> rusqlite::Result<TurnRecord> {
    Ok(TurnRecord {
        session_id: row.get(0)?,
        turn_idx: row.get(1)?,
        calls: row.get(2)?,
        ended_well: row.get(3)?,
        escalated: row.get(4)?,
        digest_hash: row.get(5)?,
    })
}
fn read_json<T: serde::de::DeserializeOwned>(
    row: &rusqlite::Row<'_>,
    index: usize,
) -> rusqlite::Result<T> {
    let value: String = row.get(index)?;
    serde_json::from_str(&value)
        .map_err(|e| rusqlite::Error::FromSqlConversionFailure(index, Type::Text, Box::new(e)))
}
fn read_event(row: &rusqlite::Row<'_>) -> rusqlite::Result<EventRecord> {
    Ok(EventRecord {
        id: row.get(0)?,
        kind: row.get(1)?,
        session_id: row.get(2)?,
        inputs: read_json(row, 3)?,
        evidence_ids: read_json(row, 4)?,
        explanation_template_id: row.get(5)?,
        params: read_json(row, 6)?,
        created_at: row.get::<_, Option<i64>>(7)?.map(Timestamp),
    })
}
fn read_u64(row: &rusqlite::Row<'_>, index: usize) -> rusqlite::Result<u64> {
    let value: i64 = row.get(index)?;
    u64::try_from(value)
        .map_err(|e| rusqlite::Error::FromSqlConversionFailure(index, Type::Integer, Box::new(e)))
}
fn read_ledger(row: &rusqlite::Row<'_>) -> rusqlite::Result<LedgerRecord> {
    Ok(LedgerRecord {
        session_id: row.get(0)?,
        usage: Usage {
            input: read_u64(row, 1)?,
            output: read_u64(row, 2)?,
            cache_read: read_u64(row, 3)?,
            cache_write: read_u64(row, 4)?,
        },
        priced_cost: row.get(5)?,
        overhead_tokens: read_u64(row, 6)?,
        overhead_cost: row.get(7)?,
    })
}
impl Store {
    fn event_columns(&self) -> &'static str {
        if self.schema_version < 2 {
            "id, kind, session_id, inputs, evidence_ids, explanation_template_id, params, NULL AS created_at"
        } else {
            EVENT_COLUMNS
        }
    }

    /// Insert or update a session, preserving its original start time.
    ///
    /// # Errors
    /// Returns an error if the path is not UTF-8 or the write fails.
    pub fn upsert_session(&self, session: &NewSession) -> Result<i64, StoreError> {
        let cwd = session.cwd.to_str().ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "session path is not UTF-8",
            )
        })?;
        Ok(self.conn.query_row("INSERT INTO sessions (host, agent_home, session_id, repo_key, cwd, started_at, ended_at, fold_status, shadow, integration_level) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10) ON CONFLICT(host, agent_home, session_id) DO UPDATE SET repo_key=excluded.repo_key, cwd=excluded.cwd, ended_at=excluded.ended_at, fold_status=excluded.fold_status, shadow=excluded.shadow, integration_level=excluded.integration_level RETURNING id", params![session.key.host.as_str(), session.key.agent_home, session.key.session_id, session.repo_key.0, cwd, session.started.0, session.ended.map(|v| v.0), session.fold_status, session.shadow, session.integration_level], |row| row.get(0))?)
    }
    /// Find a session by local identifier.
    ///
    /// # Errors
    /// Returns an error if querying or decoding fails.
    pub fn get_session(&self, id: i64) -> Result<Option<SessionRecord>, StoreError> {
        Ok(self
            .conn
            .query_row(
                &format!("SELECT {SESSION_COLUMNS} FROM sessions WHERE id=?1"),
                [id],
                read_session,
            )
            .optional()?)
    }
    /// Find a session by host-scoped identity.
    ///
    /// # Errors
    /// Returns an error if querying or decoding fails.
    pub fn find_session(&self, key: &SessionKey) -> Result<Option<SessionRecord>, StoreError> {
        Ok(self.conn.query_row(&format!("SELECT {SESSION_COLUMNS} FROM sessions WHERE host=?1 AND agent_home=?2 AND session_id=?3"), params![key.host.as_str(), key.agent_home, key.session_id], read_session).optional()?)
    }
    /// Set a session's end time.
    ///
    /// # Errors
    /// Returns an error if the write fails or the session does not exist.
    pub fn end_session(&self, id: i64, ended: Timestamp) -> Result<(), StoreError> {
        self.update_session(
            "UPDATE sessions SET ended_at=?1 WHERE id=?2",
            params![ended.0, id],
        )
    }
    /// Set a session's fold state.
    ///
    /// # Errors
    /// Returns an error if the write fails or the session does not exist.
    pub fn set_session_fold_status(&self, id: i64, status: &str) -> Result<(), StoreError> {
        self.update_session(
            "UPDATE sessions SET fold_status=?1 WHERE id=?2",
            params![status, id],
        )
    }
    /// Set a session's shadow flag.
    ///
    /// # Errors
    /// Returns an error if the write fails or the session does not exist.
    pub fn set_session_shadow(&self, id: i64, shadow: bool) -> Result<(), StoreError> {
        self.update_session(
            "UPDATE sessions SET shadow=?1 WHERE id=?2",
            params![shadow, id],
        )
    }
    /// Set a session's integration level.
    ///
    /// # Errors
    /// Returns an error if the write fails or the session does not exist.
    pub fn set_integration_level(&self, id: i64, level: &str) -> Result<(), StoreError> {
        self.update_session(
            "UPDATE sessions SET integration_level=?1 WHERE id=?2",
            params![level, id],
        )
    }
    fn update_session<P: rusqlite::Params>(&self, sql: &str, params: P) -> Result<(), StoreError> {
        if self.conn.execute(sql, params)? == 0 {
            return Err(rusqlite::Error::QueryReturnedNoRows.into());
        }
        Ok(())
    }
    /// List sessions for a repository, newest first; negative limits are unbounded.
    ///
    /// # Errors
    /// Returns an error if querying or decoding fails.
    pub fn list_sessions_by_repo(
        &self,
        key: &RepoKey,
        limit: i64,
    ) -> Result<Vec<SessionRecord>, StoreError> {
        let mut stmt = self.conn.prepare(&format!("SELECT {SESSION_COLUMNS} FROM sessions WHERE repo_key=?1 ORDER BY started_at DESC, id DESC LIMIT ?2"))?;
        let rows = stmt.query_map(params![key.0, limit], read_session)?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }
    /// Insert or update a turn.
    ///
    /// # Errors
    /// Returns an error if the write fails.
    pub fn upsert_turn(&self, turn: &TurnRecord) -> Result<(), StoreError> {
        self.conn.execute("INSERT INTO turns (session_id, turn_idx, calls, ended_well, escalated, digest_hash) VALUES (?1, ?2, ?3, ?4, ?5, ?6) ON CONFLICT(session_id, turn_idx) DO UPDATE SET calls=excluded.calls, ended_well=excluded.ended_well, escalated=excluded.escalated, digest_hash=excluded.digest_hash", params![turn.session_id, turn.turn_idx, turn.calls, turn.ended_well, turn.escalated, turn.digest_hash])?;
        Ok(())
    }
    /// List turns in index order.
    ///
    /// # Errors
    /// Returns an error if querying or decoding fails.
    pub fn list_turns(&self, session_id: i64) -> Result<Vec<TurnRecord>, StoreError> {
        let mut stmt = self.conn.prepare("SELECT session_id, turn_idx, calls, ended_well, escalated, digest_hash FROM turns WHERE session_id=?1 ORDER BY turn_idx")?;
        let rows = stmt.query_map([session_id], read_turn)?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }
    /// Insert an action event and return its identifier.
    ///
    /// # Errors
    /// Returns an error if encoding or writing fails.
    pub fn insert_event(&self, event: &NewEvent) -> Result<i64, StoreError> {
        self.insert_event_with_time(event, None)
    }
    /// Insert an action with its daemon-supplied Unix-millisecond timestamp.
    ///
    /// # Errors
    /// Returns an error if encoding or writing fails.
    pub fn insert_event_at(&self, event: &NewEvent, at: Timestamp) -> Result<i64, StoreError> {
        self.insert_event_with_time(event, Some(at))
    }
    /// Advance the latest hook observation to the session's last known activity time.
    ///
    /// Keeps coverage rows deduplicated and never moves a timestamp backwards. A session
    /// without hook observations is unchanged. Intended for the daemon's periodic flusher.
    ///
    /// # Errors
    /// Returns an error if the database update fails.
    pub fn refresh_hook_observation(
        &self,
        session_id: i64,
        at: Timestamp,
    ) -> Result<(), StoreError> {
        self.conn.execute(
            "UPDATE events SET created_at=?2 WHERE id=(SELECT id FROM events WHERE session_id=?1 AND kind='hook.seen' ORDER BY id DESC LIMIT 1) AND (created_at IS NULL OR created_at<?2)",
            params![session_id, at.0],
        )?;
        Ok(())
    }
    fn insert_event_with_time(
        &self,
        event: &NewEvent,
        at: Option<Timestamp>,
    ) -> Result<i64, StoreError> {
        let inputs = serde_json::to_string(&event.inputs)?;
        let evidence = serde_json::to_string(&event.evidence_ids)?;
        let parameters = serde_json::to_string(&event.params)?;
        Ok(self.conn.query_row("INSERT INTO events (kind, session_id, inputs, evidence_ids, explanation_template_id, params, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7) RETURNING id", params![event.kind, event.session_id, inputs, evidence, event.explanation_template_id, parameters, at.map(|t| t.0)], |row| row.get(0))?)
    }
    /// Find an event by identifier.
    ///
    /// # Errors
    /// Returns an error if querying or decoding fails.
    pub fn get_event(&self, id: i64) -> Result<Option<EventRecord>, StoreError> {
        Ok(self
            .conn
            .query_row(
                &format!("SELECT {} FROM events WHERE id=?1", self.event_columns()),
                [id],
                read_event,
            )
            .optional()?)
    }
    /// List a session's events in insertion order.
    ///
    /// # Errors
    /// Returns an error if querying or decoding fails.
    pub fn list_events_by_session(&self, session_id: i64) -> Result<Vec<EventRecord>, StoreError> {
        let mut stmt = self.conn.prepare(&format!(
            "SELECT {} FROM events WHERE session_id=?1 ORDER BY id",
            self.event_columns()
        ))?;
        let rows = stmt.query_map([session_id], read_event)?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }
    /// List recent events, newest first; negative limits are unbounded.
    ///
    /// # Errors
    /// Returns an error if querying or decoding fails.
    pub fn list_recent_events(&self, limit: i64) -> Result<Vec<EventRecord>, StoreError> {
        let mut stmt = self.conn.prepare(&format!(
            "SELECT {} FROM events ORDER BY id DESC LIMIT ?1",
            self.event_columns()
        ))?;
        let rows = stmt.query_map([limit], read_event)?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }
    /// Insert or update a session ledger.
    ///
    /// # Errors
    /// Returns an error if a token count exceeds SQLite's integer range or the write fails.
    pub fn upsert_ledger(&self, ledger: &LedgerRecord) -> Result<(), StoreError> {
        let input = i64::try_from(ledger.usage.input).map_err(|_| StoreError::TokenOverflow)?;
        let output = i64::try_from(ledger.usage.output).map_err(|_| StoreError::TokenOverflow)?;
        let read = i64::try_from(ledger.usage.cache_read).map_err(|_| StoreError::TokenOverflow)?;
        let write =
            i64::try_from(ledger.usage.cache_write).map_err(|_| StoreError::TokenOverflow)?;
        let overhead =
            i64::try_from(ledger.overhead_tokens).map_err(|_| StoreError::TokenOverflow)?;
        self.conn.execute("INSERT INTO ledger (session_id, input_tokens, output_tokens, cache_read_tokens, cache_write_tokens, priced_cost, overhead_tokens, overhead_cost) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8) ON CONFLICT(session_id) DO UPDATE SET input_tokens=excluded.input_tokens, output_tokens=excluded.output_tokens, cache_read_tokens=excluded.cache_read_tokens, cache_write_tokens=excluded.cache_write_tokens, priced_cost=excluded.priced_cost, overhead_tokens=excluded.overhead_tokens, overhead_cost=excluded.overhead_cost", params![ledger.session_id, input, output, read, write, ledger.priced_cost, overhead, ledger.overhead_cost])?;
        Ok(())
    }
    /// Find a session ledger.
    ///
    /// # Errors
    /// Returns an error if querying or decoding fails.
    pub fn get_ledger(&self, session_id: i64) -> Result<Option<LedgerRecord>, StoreError> {
        Ok(self.conn.query_row("SELECT session_id, input_tokens, output_tokens, cache_read_tokens, cache_write_tokens, priced_cost, overhead_tokens, overhead_cost FROM ledger WHERE session_id=?1", [session_id], read_ledger).optional()?)
    }
}
