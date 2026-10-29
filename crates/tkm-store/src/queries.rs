//! Typed store records and queries.
use crate::{Store, StoreError};
use serde::{Deserialize, Serialize};
use tkm_core::model::{RepoKey, Timestamp};

/// Repository metadata and delegated-run count.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Repo {
    /// Opaque repository identity.
    pub repo_key: RepoKey,
    /// Human-readable name.
    pub display_name: String,
    /// First observation.
    pub first_seen: Timestamp,
    /// Latest observation.
    pub last_seen: Timestamp,
    /// Number of delegated runs.
    pub delegated_runs: i64,
}

impl Store {
    /// Insert or replace repository metadata.
    ///
    /// # Errors
    /// Returns an error if the write fails.
    pub fn upsert_repo(&self, repo: &Repo) -> Result<(), StoreError> {
        let _ = repo;
        unimplemented!()
    }
    /// Find a repository by identity.
    ///
    /// # Errors
    /// Returns an error if the query fails.
    pub fn get_repo(&self, key: &RepoKey) -> Result<Option<Repo>, StoreError> {
        let _ = key;
        unimplemented!()
    }
    /// List repositories ordered by identity.
    ///
    /// # Errors
    /// Returns an error if the query fails.
    pub fn list_repos(&self) -> Result<Vec<Repo>, StoreError> {
        unimplemented!()
    }
    /// Increment the delegated-run count for an existing repository.
    ///
    /// # Errors
    /// Returns an error if the write fails.
    pub fn increment_delegated_runs(&self, key: &RepoKey) -> Result<(), StoreError> {
        let _ = key;
        unimplemented!()
    }
}
