//! Durable free-model probe cadence and availability.
use crate::{Store, StoreError};
use rusqlite::{OptionalExtension, params};

/// Latest observed availability of a free model slug.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FreeModelProbe {
    /// Configured model slug, rather than the response's model version.
    pub model: String,
    /// Whether the probe produced a valid answer.
    pub succeeded: bool,
    /// Unix milliseconds of the attempt.
    pub probed_at: i64,
}
impl Store {
    /// Atomically claim a daily probe. A crash after claiming delays retry until the next day.
    ///
    /// # Errors
    /// Returns database write errors; callers must skip probing if the claim fails.
    pub fn claim_free_model_probe(&self, now_ms: i64) -> Result<bool, StoreError> {
        Ok(self.conn.execute("INSERT INTO free_model_probe_schedule(id,last_probed) VALUES (1,?1) ON CONFLICT(id) DO UPDATE SET last_probed=excluded.last_probed WHERE excluded.last_probed-free_model_probe_schedule.last_probed>=86400000", [now_ms])? > 0)
    }
    /// Last daily probe claim, in Unix milliseconds.
    ///
    /// # Errors
    /// Returns database read errors.
    pub fn last_free_model_probe(&self) -> Result<Option<i64>, StoreError> {
        Ok(self
            .conn
            .query_row(
                "SELECT last_probed FROM free_model_probe_schedule WHERE id=1",
                [],
                |r| r.get(0),
            )
            .optional()?)
    }
    /// Store a model's probe result without modifying configured chain order.
    ///
    /// # Errors
    /// Returns database write errors.
    pub fn record_free_model_probe(
        &self,
        model: &str,
        succeeded: bool,
        probed_at: i64,
    ) -> Result<(), StoreError> {
        self.conn.execute("INSERT INTO free_model_probes(model,succeeded,probed_at) VALUES (?1,?2,?3) ON CONFLICT(model) DO UPDATE SET succeeded=excluded.succeeded,probed_at=excluded.probed_at", params![model,succeeded,probed_at])?;
        Ok(())
    }
    /// Read durable availability for restoring the live decision client after restart.
    ///
    /// # Errors
    /// Returns database read errors.
    pub fn free_model_probes(&self) -> Result<Vec<FreeModelProbe>, StoreError> {
        if self.schema_version < 7 {
            return Ok(Vec::new());
        }
        let mut statement = self
            .conn
            .prepare("SELECT model,succeeded,probed_at FROM free_model_probes ORDER BY model")?;
        let rows = statement.query_map([], |r| {
            Ok(FreeModelProbe {
                model: r.get(0)?,
                succeeded: r.get(1)?,
                probed_at: r.get(2)?,
            })
        })?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }
}
