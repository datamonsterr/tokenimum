//! Typed store records and queries.
use crate::{Store, StoreError};
use rusqlite::{OptionalExtension, params};
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

fn read_repo(row: &rusqlite::Row<'_>) -> rusqlite::Result<Repo> {
    Ok(Repo {
        repo_key: RepoKey(row.get(0)?),
        display_name: row.get(1)?,
        first_seen: Timestamp(row.get(2)?),
        last_seen: Timestamp(row.get(3)?),
        delegated_runs: row.get(4)?,
    })
}

impl Store {
    /// Insert or update repository metadata, preserving the observation range and run count.
    ///
    /// # Errors
    /// Returns an error if the write fails.
    pub fn upsert_repo(&self, repo: &Repo) -> Result<(), StoreError> {
        self.conn.execute(
            "INSERT INTO repos (repo_key, display_name, first_seen, last_seen, delegated_runs) VALUES (?1, ?2, ?3, ?4, ?5) ON CONFLICT(repo_key) DO UPDATE SET display_name = excluded.display_name, first_seen = MIN(repos.first_seen, excluded.first_seen), last_seen = MAX(repos.last_seen, excluded.last_seen)",
            params![repo.repo_key.0, repo.display_name, repo.first_seen.0, repo.last_seen.0, repo.delegated_runs],
        )?;
        Ok(())
    }
    /// Find a repository by identity.
    ///
    /// # Errors
    /// Returns an error if the query fails.
    pub fn get_repo(&self, key: &RepoKey) -> Result<Option<Repo>, StoreError> {
        Ok(self.conn.query_row("SELECT repo_key, display_name, first_seen, last_seen, delegated_runs FROM repos WHERE repo_key = ?1", [&key.0], read_repo).optional()?)
    }
    /// List repositories ordered by identity.
    ///
    /// # Errors
    /// Returns an error if the query fails.
    pub fn list_repos(&self) -> Result<Vec<Repo>, StoreError> {
        let mut statement = self.conn.prepare("SELECT repo_key, display_name, first_seen, last_seen, delegated_runs FROM repos ORDER BY repo_key")?;
        let rows = statement.query_map([], read_repo)?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }
    /// Increment the delegated-run count for an existing repository.
    ///
    /// # Errors
    /// Returns an error if the write fails or the repository does not exist.
    pub fn increment_delegated_runs(&self, key: &RepoKey) -> Result<(), StoreError> {
        let changed = self.conn.execute(
            "UPDATE repos SET delegated_runs = delegated_runs + 1 WHERE repo_key = ?1",
            [&key.0],
        )?;
        if changed == 0 {
            return Err(rusqlite::Error::QueryReturnedNoRows.into());
        }
        Ok(())
    }
}
