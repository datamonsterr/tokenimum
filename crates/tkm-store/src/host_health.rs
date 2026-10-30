//! Durable doctor health overrides, separate from person-owned configuration.

use crate::{Store, StoreError};
use tkm_core::model::Host;

impl Store {
    /// Record hook/shim health. A passing check removes a previous fold-only override.
    ///
    /// # Errors
    /// Returns database errors; callers should fail open.
    pub fn record_host_health(&self, host: Host, healthy: bool) -> Result<(), StoreError> {
        if healthy {
            self.conn
                .execute("DELETE FROM host_health WHERE host=?1", [host.as_str()])?;
        } else {
            self.conn.execute(
                "INSERT OR REPLACE INTO host_health(host,fold_only) VALUES (?1,1)",
                [host.as_str()],
            )?;
        }
        Ok(())
    }

    /// Whether doctor has forced this host to fold-only.
    ///
    /// # Errors
    /// Returns database read errors.
    pub fn host_fold_only(&self, host: Host) -> Result<bool, StoreError> {
        if self.schema_version < 5 {
            return Ok(false);
        }
        Ok(self.conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM host_health WHERE host=?1)",
            [host.as_str()],
            |row| row.get(0),
        )?)
    }
}
