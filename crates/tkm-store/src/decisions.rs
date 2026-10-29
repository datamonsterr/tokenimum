//! Typed decision, calibration, and installation records and queries.
use crate::{Store, StoreError};
use rusqlite::{OptionalExtension, params, types::Type};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use tkm_core::model::{Host, RepoKey, Timestamp};

/// Decision data for insertion.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NewDecision {
    /// Model version.
    pub model: String,
    /// Decision purpose.
    pub purpose: String,
    /// Decision questions.
    pub questions: serde_json::Value,
    /// Decision answers.
    pub answers: serde_json::Value,
    /// Decision cost in US dollars.
    pub cost_usd: Option<f64>,
    /// Latency in milliseconds.
    pub latency_ms: u32,
    /// Whether the decision was shadowed.
    pub shadow: bool,
}

/// Persisted decision and its observed outcome.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DecisionRecord {
    /// Local decision identifier.
    pub id: i64,
    /// Model version.
    pub model: String,
    /// Decision purpose.
    pub purpose: String,
    /// Decision questions.
    pub questions: serde_json::Value,
    /// Decision answers.
    pub answers: serde_json::Value,
    /// Decision cost in US dollars.
    pub cost_usd: Option<f64>,
    /// Latency in milliseconds.
    pub latency_ms: u32,
    /// Whether the decision was shadowed.
    pub shadow: bool,
    /// Observed outcome, if labeled.
    pub outcome_label: Option<String>,
    /// Decision time in Unix milliseconds, absent for historical writes.
    #[serde(default)]
    pub created_at: Option<Timestamp>,
    /// Repository known when the request was sent.
    #[serde(default)]
    pub repo_key: Option<RepoKey>,
    /// Local owning session, when known.
    #[serde(default)]
    pub session_id: Option<i64>,
    /// Redacted request state exactly as sent, never raw logs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub digest: Option<String>,
}

/// Thresholds and fit metrics for a model and purpose.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CalibrationRecord {
    /// Model version.
    pub model_version: String,
    /// Decision purpose.
    pub purpose: String,
    /// Calibrated thresholds.
    pub thresholds: serde_json::Value,
    /// Number of labeled samples.
    pub sample_count: u32,
    /// Measured precision.
    pub precision: f64,
    /// Measured recall.
    pub recall: f64,
}

/// Installation manifest read by `tkm uninstall` to restore only entries tokenimum wrote.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InstallRecord {
    /// Harness identity.
    pub host: Host,
    /// Harness home path.
    pub agent_home: PathBuf,
    /// Paths written by tokenimum.
    pub files_written: Vec<PathBuf>,
    /// Identifiers of entries written by tokenimum.
    pub entry_ids: Vec<String>,
    /// Backup file paths.
    pub backup_paths: Vec<PathBuf>,
    /// Hashes of generated shims.
    pub shim_hashes: BTreeMap<String, String>,
}

const DECISION_COLUMNS: &str = "id, model, purpose, questions, answers, cost, latency_ms, shadow, outcome_label, created_at, repo_key, session_id, digest";
const CALIBRATION_COLUMNS: &str =
    "model_version, purpose, thresholds, sample_count, precision, recall";
const INSTALL_COLUMNS: &str =
    "host, agent_home, files_written, entry_ids, backup_paths, shim_hashes";

fn read_json<T: serde::de::DeserializeOwned>(
    row: &rusqlite::Row<'_>,
    index: usize,
) -> rusqlite::Result<T> {
    let value: String = row.get(index)?;
    serde_json::from_str(&value).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(index, Type::Text, Box::new(error))
    })
}
fn read_decision(row: &rusqlite::Row<'_>) -> rusqlite::Result<DecisionRecord> {
    Ok(DecisionRecord {
        id: row.get(0)?,
        model: row.get(1)?,
        purpose: row.get(2)?,
        questions: read_json(row, 3)?,
        answers: read_json(row, 4)?,
        cost_usd: row.get(5)?,
        latency_ms: row.get(6)?,
        shadow: row.get(7)?,
        outcome_label: row.get(8)?,
        created_at: row.get::<_, Option<i64>>(9)?.map(Timestamp),
        repo_key: row.get::<_, Option<String>>(10)?.map(RepoKey),
        session_id: row.get(11)?,
        digest: row.get(12)?,
    })
}
fn read_calibration(row: &rusqlite::Row<'_>) -> rusqlite::Result<CalibrationRecord> {
    Ok(CalibrationRecord {
        model_version: row.get(0)?,
        purpose: row.get(1)?,
        thresholds: read_json(row, 2)?,
        sample_count: row.get(3)?,
        precision: row.get(4)?,
        recall: row.get(5)?,
    })
}
fn read_install(row: &rusqlite::Row<'_>) -> rusqlite::Result<InstallRecord> {
    let host: String = row.get(0)?;
    Ok(InstallRecord {
        host: host.parse::<Host>().map_err(|error| {
            rusqlite::Error::FromSqlConversionFailure(0, Type::Text, Box::new(error))
        })?,
        agent_home: PathBuf::from(row.get::<_, String>(1)?),
        files_written: read_json(row, 2)?,
        entry_ids: read_json(row, 3)?,
        backup_paths: read_json(row, 4)?,
        shim_hashes: read_json(row, 5)?,
    })
}
fn path_text(path: &Path) -> Result<&str, StoreError> {
    path.to_str().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "installation path is not UTF-8",
        )
        .into()
    })
}

impl Store {
    /// Persist one serialized decision attempt, including unknown cost.
    ///
    /// # Errors
    /// Returns an error if storage or serialization fails.
    pub fn insert_spend_attempt(&self, record: &serde_json::Value) -> Result<(), StoreError> {
        self.conn.execute(
            "INSERT INTO spend_attempts (record) VALUES (?1)",
            [serde_json::to_string(record)?],
        )?;
        Ok(())
    }

    /// Read all durable decision attempts in insertion order.
    ///
    /// # Errors
    /// Returns an error if storage or decoding fails.
    pub fn list_spend_attempts(&self) -> Result<Vec<serde_json::Value>, StoreError> {
        if self.schema_version < 4 {
            return Ok(Vec::new());
        }
        let mut query = self
            .conn
            .prepare("SELECT record FROM spend_attempts ORDER BY id")?;
        let rows = query.query_map([], |row| read_json(row, 0))?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// Last decision id already present when attempt persistence was introduced.
    ///
    /// # Errors
    /// Returns an error if the migration marker cannot be read.
    pub fn legacy_spend_max_id(&self) -> Result<i64, StoreError> {
        if self.schema_version < 4 {
            return Ok(i64::MAX);
        }
        Ok(self.conn.query_row(
            "SELECT legacy_decision_max FROM spend_migration",
            [],
            |row| row.get(0),
        )?)
    }

    fn decision_columns(&self) -> &'static str {
        if self.schema_version < 2 {
            "id, model, purpose, questions, answers, cost, latency_ms, shadow, outcome_label, NULL AS created_at, NULL AS repo_key, NULL AS session_id, NULL AS digest"
        } else if self.schema_version >= 4 {
            "id, model, purpose, questions, answers, cost_usd, latency_ms, shadow, outcome_label, created_at, repo_key, session_id, digest"
        } else {
            DECISION_COLUMNS
        }
    }

    /// Insert a decision and return its identifier.
    ///
    /// # Errors
    /// Returns an error if encoding or writing fails.
    pub fn insert_decision(&self, decision: &NewDecision) -> Result<i64, StoreError> {
        self.insert_decision_with_context(decision, None, None, None, None)
    }
    /// Insert a decision with optional reporting context.
    /// `digest` must already be the redacted state sent to the decision client.
    ///
    /// # Errors
    /// Returns an error if encoding or writing fails.
    pub fn insert_decision_with_context(
        &self,
        decision: &NewDecision,
        created_at: Option<Timestamp>,
        repo: Option<&RepoKey>,
        session_id: Option<i64>,
        digest: Option<&str>,
    ) -> Result<i64, StoreError> {
        let questions = serde_json::to_string(&decision.questions)?;
        let answers = serde_json::to_string(&decision.answers)?;
        Ok(self.conn.query_row("INSERT INTO decisions (model, purpose, questions, answers, cost, latency_ms, shadow, created_at, repo_key, session_id, digest, cost_usd) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12) RETURNING id", params![decision.model, decision.purpose, questions, answers, decision.cost_usd.unwrap_or(0.0), i64::from(decision.latency_ms), decision.shadow, created_at.map(|t| t.0), repo.map(|r| &r.0), session_id, digest, decision.cost_usd], |row| row.get(0))?)
    }
    /// Set an observed outcome label.
    ///
    /// # Errors
    /// Returns an error if the write fails or the decision does not exist.
    pub fn set_decision_outcome(&self, id: i64, label: &str) -> Result<(), StoreError> {
        if self.conn.execute(
            "UPDATE decisions SET outcome_label=?1 WHERE id=?2",
            params![label, id],
        )? == 0
        {
            return Err(rusqlite::Error::QueryReturnedNoRows.into());
        }
        Ok(())
    }
    /// Find a decision by identifier.
    ///
    /// # Errors
    /// Returns an error if querying or decoding fails.
    pub fn get_decision(&self, id: i64) -> Result<Option<DecisionRecord>, StoreError> {
        Ok(self
            .conn
            .query_row(
                &format!(
                    "SELECT {} FROM decisions WHERE id=?1",
                    self.decision_columns()
                ),
                [id],
                read_decision,
            )
            .optional()?)
    }
    /// List recent decisions, newest first; negative limits are unbounded.
    ///
    /// # Errors
    /// Returns an error if querying or decoding fails.
    pub fn list_recent_decisions(&self, limit: i64) -> Result<Vec<DecisionRecord>, StoreError> {
        let mut statement = self.conn.prepare(&format!(
            "SELECT {} FROM decisions ORDER BY id DESC LIMIT ?1",
            self.decision_columns()
        ))?;
        let rows = statement.query_map([limit], read_decision)?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }
    /// List matching decisions, newest first; negative limits are unbounded.
    ///
    /// # Errors
    /// Returns an error if querying or decoding fails.
    pub fn list_decisions_by_model_purpose(
        &self,
        model: &str,
        purpose: &str,
        limit: i64,
    ) -> Result<Vec<DecisionRecord>, StoreError> {
        let mut statement = self.conn.prepare(&format!(
            "SELECT {} FROM decisions WHERE model=?1 AND purpose=?2 ORDER BY id DESC LIMIT ?3",
            self.decision_columns()
        ))?;
        let rows = statement.query_map(params![model, purpose, limit], read_decision)?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }
    /// Insert or update calibration for a model and purpose.
    ///
    /// # Errors
    /// Returns an error if encoding or writing fails.
    pub fn upsert_calibration(&self, calibration: &CalibrationRecord) -> Result<(), StoreError> {
        let thresholds = serde_json::to_string(&calibration.thresholds)?;
        self.conn.execute("INSERT INTO calibration (model_version, purpose, thresholds, sample_count, precision, recall) VALUES (?1, ?2, ?3, ?4, ?5, ?6) ON CONFLICT(model_version, purpose) DO UPDATE SET thresholds=excluded.thresholds, sample_count=excluded.sample_count, precision=excluded.precision, recall=excluded.recall", params![calibration.model_version, calibration.purpose, thresholds, i64::from(calibration.sample_count), calibration.precision, calibration.recall])?;
        Ok(())
    }
    /// Find calibration for a model and purpose.
    ///
    /// # Errors
    /// Returns an error if querying or decoding fails.
    pub fn get_calibration(
        &self,
        model_version: &str,
        purpose: &str,
    ) -> Result<Option<CalibrationRecord>, StoreError> {
        Ok(self.conn.query_row(&format!("SELECT {CALIBRATION_COLUMNS} FROM calibration WHERE model_version=?1 AND purpose=?2"), params![model_version, purpose], read_calibration).optional()?)
    }
    /// List calibration records in model and purpose order.
    ///
    /// # Errors
    /// Returns an error if querying or decoding fails.
    pub fn list_calibration(&self) -> Result<Vec<CalibrationRecord>, StoreError> {
        let mut statement = self.conn.prepare(&format!(
            "SELECT {CALIBRATION_COLUMNS} FROM calibration ORDER BY model_version, purpose"
        ))?;
        let rows = statement.query_map([], read_calibration)?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }
    /// Insert or update an installation manifest.
    ///
    /// # Errors
    /// Returns an error if a path is not UTF-8, encoding fails, or the write fails.
    pub fn upsert_install(&self, install: &InstallRecord) -> Result<(), StoreError> {
        let home = path_text(&install.agent_home)?;
        let files = serde_json::to_string(&install.files_written)?;
        let entries = serde_json::to_string(&install.entry_ids)?;
        let backups = serde_json::to_string(&install.backup_paths)?;
        let hashes = serde_json::to_string(&install.shim_hashes)?;
        self.conn.execute("INSERT INTO installs (host, agent_home, files_written, entry_ids, backup_paths, shim_hashes) VALUES (?1, ?2, ?3, ?4, ?5, ?6) ON CONFLICT(host, agent_home) DO UPDATE SET files_written=excluded.files_written, entry_ids=excluded.entry_ids, backup_paths=excluded.backup_paths, shim_hashes=excluded.shim_hashes", params![install.host.as_str(), home, files, entries, backups, hashes])?;
        Ok(())
    }
    /// Find an installation by host and home.
    ///
    /// # Errors
    /// Returns an error if the path is not UTF-8, querying fails, or decoding fails.
    pub fn get_install(
        &self,
        host: Host,
        agent_home: &Path,
    ) -> Result<Option<InstallRecord>, StoreError> {
        Ok(self
            .conn
            .query_row(
                &format!("SELECT {INSTALL_COLUMNS} FROM installs WHERE host=?1 AND agent_home=?2"),
                params![host.as_str(), path_text(agent_home)?],
                read_install,
            )
            .optional()?)
    }
    /// Delete an installation; return whether it existed.
    ///
    /// # Errors
    /// Returns an error if the path is not UTF-8 or the write fails.
    pub fn delete_install(&self, host: Host, agent_home: &Path) -> Result<bool, StoreError> {
        Ok(self.conn.execute(
            "DELETE FROM installs WHERE host=?1 AND agent_home=?2",
            params![host.as_str(), path_text(agent_home)?],
        )? > 0)
    }
    /// List installation manifests in host and home order.
    ///
    /// # Errors
    /// Returns an error if querying or decoding fails.
    pub fn list_installs(&self) -> Result<Vec<InstallRecord>, StoreError> {
        let mut statement = self.conn.prepare(&format!(
            "SELECT {INSTALL_COLUMNS} FROM installs ORDER BY host, agent_home"
        ))?;
        let rows = statement.query_map([], read_install)?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }
}
