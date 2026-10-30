//! Durable pruning readiness and delegation history.

use crate::{SessionKey, Store, StoreError};
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

impl Store {
    /// Record a delegated session once, retaining its first observed start and lifetime count.
    /// The transaction makes outbox retries and daemon restarts idempotent.
    ///
    /// # Errors
    /// Returns an error if the repository is absent or the transaction fails.
    pub fn record_delegated_run(
        &self,
        repo: &RepoKey,
        key: &SessionKey,
        at: Timestamp,
    ) -> Result<(), StoreError> {
        let tx = self.conn.unchecked_transaction()?;
        let inserted = tx.execute("INSERT INTO delegated_run_history (repo_key, host, agent_home, session_id, ran_at) VALUES (?1, ?2, ?3, ?4, ?5) ON CONFLICT(repo_key, host, agent_home, session_id) DO NOTHING", params![repo.0, key.host.as_str(), key.agent_home, key.session_id, at.0])?;
        if inserted > 0 {
            tx.execute(
                "UPDATE repos SET delegated_runs = delegated_runs + 1 WHERE repo_key = ?1",
                [&repo.0],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    /// Latest delegated start in the inclusive last 30 days, excluding future observations.
    /// Historical lifetime counts without timestamps do not imply recent delegation.
    ///
    /// # Errors
    /// Returns an error when history cannot be queried.
    pub fn last_delegated_run_within(
        &self,
        repo: &RepoKey,
        now: Timestamp,
    ) -> Result<Option<Timestamp>, StoreError> {
        let at: Option<i64> = self.conn.query_row("SELECT MAX(ran_at) FROM delegated_run_history WHERE repo_key = ?1 AND ran_at >= ?2 AND ran_at <= ?3", params![repo.0, now.0.saturating_sub(2_592_000_000), now.0], |row| row.get(0))?;
        Ok(at.map(Timestamp))
    }
}
