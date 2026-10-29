//! Typed decision, calibration, and installation records and queries.
use crate::{Store, StoreError};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use tkm_core::model::Host;

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
    pub cost_usd: f64,
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
    pub cost_usd: f64,
    /// Latency in milliseconds.
    pub latency_ms: u32,
    /// Whether the decision was shadowed.
    pub shadow: bool,
    /// Observed outcome, if labeled.
    pub outcome_label: Option<String>,
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

impl Store {
    /// Insert a decision and return its identifier.
    ///
    /// # Errors
    /// Returns an error if encoding, decoding, or the database operation fails.
    pub fn insert_decision(&self, decision: &NewDecision) -> Result<i64, StoreError> {
        unimplemented!()
    }
    /// Set an observed outcome label.
    ///
    /// # Errors
    /// Returns an error if encoding, decoding, or the database operation fails.
    pub fn set_decision_outcome(&self, id: i64, label: &str) -> Result<(), StoreError> {
        unimplemented!()
    }
    /// Find a decision by identifier.
    ///
    /// # Errors
    /// Returns an error if encoding, decoding, or the database operation fails.
    pub fn get_decision(&self, id: i64) -> Result<Option<DecisionRecord>, StoreError> {
        unimplemented!()
    }
    /// List recent decisions, newest first; negative limits are unbounded.
    ///
    /// # Errors
    /// Returns an error if encoding, decoding, or the database operation fails.
    pub fn list_recent_decisions(&self, limit: i64) -> Result<Vec<DecisionRecord>, StoreError> {
        unimplemented!()
    }
    /// List matching decisions, newest first; negative limits are unbounded.
    ///
    /// # Errors
    /// Returns an error if encoding, decoding, or the database operation fails.
    pub fn list_decisions_by_model_purpose(
        &self,
        model: &str,
        purpose: &str,
        limit: i64,
    ) -> Result<Vec<DecisionRecord>, StoreError> {
        unimplemented!()
    }
    /// Insert or update calibration for a model and purpose.
    ///
    /// # Errors
    /// Returns an error if encoding, decoding, or the database operation fails.
    pub fn upsert_calibration(&self, calibration: &CalibrationRecord) -> Result<(), StoreError> {
        unimplemented!()
    }
    /// Find calibration for a model and purpose.
    ///
    /// # Errors
    /// Returns an error if encoding, decoding, or the database operation fails.
    pub fn get_calibration(
        &self,
        model_version: &str,
        purpose: &str,
    ) -> Result<Option<CalibrationRecord>, StoreError> {
        unimplemented!()
    }
    /// List calibration records.
    ///
    /// # Errors
    /// Returns an error if encoding, decoding, or the database operation fails.
    pub fn list_calibration(&self) -> Result<Vec<CalibrationRecord>, StoreError> {
        unimplemented!()
    }
    /// Insert or update an installation manifest.
    ///
    /// # Errors
    /// Returns an error if encoding, decoding, or the database operation fails.
    pub fn upsert_install(&self, install: &InstallRecord) -> Result<(), StoreError> {
        unimplemented!()
    }
    /// Find an installation by host and home.
    ///
    /// # Errors
    /// Returns an error if encoding, decoding, or the database operation fails.
    pub fn get_install(
        &self,
        host: Host,
        agent_home: &Path,
    ) -> Result<Option<InstallRecord>, StoreError> {
        unimplemented!()
    }
    /// Delete an installation; return whether it existed.
    ///
    /// # Errors
    /// Returns an error if encoding, decoding, or the database operation fails.
    pub fn delete_install(&self, host: Host, agent_home: &Path) -> Result<bool, StoreError> {
        unimplemented!()
    }
    /// List installation manifests.
    ///
    /// # Errors
    /// Returns an error if encoding, decoding, or the database operation fails.
    pub fn list_installs(&self) -> Result<Vec<InstallRecord>, StoreError> {
        unimplemented!()
    }
}
