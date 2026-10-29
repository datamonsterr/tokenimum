//! Persistent per-session skill evidence.
use crate::{Store, StoreError};
use rusqlite::params;
use tkm_core::model::RepoKey;
use tkm_core::prune::skills::{SkillReport, SkillUsage};

impl Store {
    /// Atomically replace the evidence from one complete session.
    ///
    /// # Errors
    /// Returns an error if the evidence cannot be stored.
    pub fn replace_skill_evidence(
        &self,
        session_id: i64,
        report: &SkillReport,
    ) -> Result<(), StoreError> {
        let tx = self.conn.unchecked_transaction()?;
        tx.execute(
            "DELETE FROM skill_evidence WHERE session_id=?1",
            [session_id],
        )?;
        for (name, usage) in &report.skills {
            let loaded =
                i64::try_from(usage.loaded_sessions).map_err(|_| StoreError::TokenOverflow)?;
            let used = i64::try_from(usage.invocations).map_err(|_| StoreError::TokenOverflow)?;
            tx.execute(
                "INSERT INTO skill_evidence (session_id,name,loaded,used) VALUES (?1,?2,?3,?4)",
                params![session_id, name, loaded, used],
            )?;
        }
        tx.execute(
            "INSERT OR IGNORE INTO skill_evidence_sessions (session_id) VALUES (?1)",
            [session_id],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// Whether a session has already had skill evidence folded.
    ///
    /// # Errors
    /// Returns an error if the marker cannot be read.
    pub fn has_skill_evidence(&self, session_id: i64) -> Result<bool, StoreError> {
        if self.schema_version < 3 {
            return Ok(false);
        }
        Ok(self.conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM skill_evidence_sessions WHERE session_id=?1)",
            [session_id],
            |row| row.get(0),
        )?)
    }

    /// Aggregate stored skill evidence for a repository.
    ///
    /// # Errors
    /// Returns an error if the evidence cannot be read.
    pub fn skill_evidence(&self, repo: &RepoKey) -> Result<SkillReport, StoreError> {
        if self.schema_version < 3 {
            return Ok(SkillReport::default());
        }
        let mut stmt = self.conn.prepare("SELECT e.name, SUM(e.loaded), SUM(e.used) FROM skill_evidence e JOIN sessions s ON s.id=e.session_id WHERE s.repo_key=?1 GROUP BY e.name ORDER BY e.name")?;
        let rows = stmt.query_map([&repo.0], |row| {
            Ok((
                row.get::<_, String>(0)?,
                SkillUsage {
                    loaded_sessions: unsigned(row.get(1)?, 1)?,
                    invocations: unsigned(row.get(2)?, 2)?,
                },
            ))
        })?;
        let mut report = SkillReport::default();
        for row in rows {
            let (name, usage) = row?;
            report.skills.insert(name, usage);
        }
        Ok(report)
    }
}

fn unsigned(value: i64, column: usize) -> rusqlite::Result<u64> {
    u64::try_from(value).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(
            column,
            rusqlite::types::Type::Integer,
            Box::new(e),
        )
    })
}
