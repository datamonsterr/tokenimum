//! Durable pruning readiness and delegation history.

use crate::{Store, StoreError};
use rusqlite::{OptionalExtension, params};
use tkm_core::{
    model::{Host, RepoKey, Timestamp},
    prune::{self, PruneConfig, ToolEvidence},
};

impl Store {
    /// Start readiness at the first eligible observation, and clear it when no longer eligible.
    /// Repeated observations preserve the original deadline. Counts are checked again on update
    /// so a concurrent use cannot revive stale readiness.
    ///
    /// # Errors
    /// Returns an error when evidence cannot be read or readiness cannot be written.
    pub fn refresh_prune_readiness(
        &self,
        repo: &RepoKey,
        host: Host,
        config: &PruneConfig,
        at: Timestamp,
    ) -> Result<(), StoreError> {
        for row in self.list_tool_evidence(repo, host)? {
            let eligible = prune::eligible(
                &ToolEvidence {
                    prompts: u64::from(row.prompts_unused),
                    sessions: u64::from(row.sessions_unused),
                    ..ToolEvidence::default()
                },
                config,
            ) && !prune::is_protected(&row.tool, host, config);
            self.conn.execute("UPDATE tool_evidence SET ready_since = CASE WHEN ?4 THEN COALESCE(ready_since, ?5) ELSE NULL END WHERE repo_key = ?1 AND host = ?2 AND tool = ?3 AND prompts_unused = ?6 AND sessions_unused = ?7",
                params![repo.0, host.as_str(), row.tool, eligible, at.0, row.prompts_unused, row.sessions_unused])?;
        }
        Ok(())
    }

    /// Return the first ready time for the current unused streak, if observed.
    ///
    /// # Errors
    /// Returns an error when readiness cannot be queried.
    pub fn prune_ready_since(
        &self,
        repo: &RepoKey,
        host: Host,
        tool: &str,
    ) -> Result<Option<Timestamp>, StoreError> {
        let at: Option<i64> = self.conn.query_row("SELECT ready_since FROM tool_evidence WHERE repo_key = ?1 AND host = ?2 AND tool = ?3", params![repo.0, host.as_str(), tool], |row| row.get::<_, Option<i64>>(0)).optional()?.flatten();
        Ok(at.map(Timestamp))
    }
}
