//! Native event coverage from isolated probes or recorded live sessions.
use crate::{Store, StoreError};
use rusqlite::params;
use tkm_core::model::Host;
use tkm_proto::EventKind;

/// Last capability observation for a native host event.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HostEventProbe {
    /// Native hook or shim event name.
    pub native: String,
    /// Normalized event name.
    pub event: String,
    /// Whether the event arrived; false means idle-fold.
    pub arrived: bool,
    /// Probe time in Unix milliseconds.
    pub probed_at: i64,
}
impl Store {
    /// Record one native event's arrival independently of other events with the same kind.
    ///
    /// # Errors
    /// Returns database write errors.
    pub fn record_host_event_probe(
        &self,
        host: Host,
        native: &str,
        event: EventKind,
        arrived: bool,
        probed_at: i64,
    ) -> Result<(), StoreError> {
        self.conn.execute("INSERT INTO host_event_probes(host,native,event,arrived,probed_at) VALUES (?1,?2,?3,?4,?5) ON CONFLICT(host,native) DO UPDATE SET event=excluded.event,arrived=excluded.arrived,probed_at=excluded.probed_at", params![host.as_str(),native,event.as_str(),arrived,probed_at])?;
        Ok(())
    }
    /// Read the last per-native-event probe policy for a host.
    ///
    /// # Errors
    /// Returns database read errors.
    pub fn host_event_probes(&self, host: Host) -> Result<Vec<HostEventProbe>, StoreError> {
        if self.schema_version < 6 {
            return Ok(Vec::new());
        }
        let mut stmt = self.conn.prepare("SELECT native,event,arrived,probed_at FROM host_event_probes WHERE host=?1 ORDER BY native")?;
        let rows = stmt.query_map([host.as_str()], |r| {
            Ok(HostEventProbe {
                native: r.get(0)?,
                event: r.get(1)?,
                arrived: r.get(2)?,
                probed_at: r.get(3)?,
            })
        })?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }
    /// Native events recorded by live hooks, for hosts whose headless mode omits hooks.
    ///
    /// # Errors
    /// Returns database read errors.
    pub fn observed_host_events(&self, host: Host) -> Result<Vec<String>, StoreError> {
        let mut stmt = self.conn.prepare("SELECT DISTINCT COALESCE(json_extract(e.inputs,'$.native'),json_extract(e.inputs,'$.event')) AS native FROM events e JOIN sessions s ON s.id=e.session_id WHERE e.kind='hook.seen' AND s.host=?1 AND native IS NOT NULL ORDER BY native")?;
        let rows = stmt.query_map([host.as_str()], |r| r.get(0))?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }
}
