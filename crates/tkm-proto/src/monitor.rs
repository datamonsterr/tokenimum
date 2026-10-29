//! Push-stream messages of the daemon's monitor socket (SPEC 11.3).
//!
//! A client connects to the monitor socket (`tkmd-monitor.sock`, next to the hook socket) and
//! reads length-prefixed JSON frames (the framing of the crate root, body limit
//! [`MONITOR_MAX_FRAME`]). The first frame is always [`MonitorMessage::Hello`]. The stream is
//! read-only: the daemon ignores anything the client sends. A subscriber that cannot keep up is
//! disconnected; it then falls back to reading the database read-only.

use serde::{Deserialize, Serialize};

use crate::Host;

/// Body limit for one monitor frame (64 KiB).
pub const MONITOR_MAX_FRAME: usize = 64 * 1024;

/// File name of the monitor socket, placed in the hook socket's directory.
pub const MONITOR_SOCKET_NAME: &str = "tkmd-monitor.sock";

/// Active-session snapshot pushed on every state change.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionUpdate {
    /// Harness of the session.
    pub host: Host,
    /// Host-native session identifier.
    pub session_id: String,
    /// Repository key, if resolved.
    pub repo: Option<String>,
    /// Lifecycle phase: `started`, `active`, `ended` or `folded`.
    pub phase: String,
    /// Tool calls in the current turn.
    pub turn_calls: u32,
    /// Learned envelope for the repository, if enough good turns exist.
    pub envelope: Option<f64>,
    /// Loop-guard state: `off`, `shadow`, `watching`, `judging`, `redirected` or `escalated`.
    pub loop_state: String,
}

/// One finished decision request.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DecisionNotice {
    /// Purpose: `loop_guard`, `relevance`, `fact_filter`.
    pub purpose: String,
    /// Dated model version that answered.
    pub model: String,
    /// Round-trip latency in milliseconds.
    pub latency_ms: u64,
    /// Priced cost in US dollars.
    pub cost_usd: f64,
    /// True when the answer was recorded but not acted on.
    pub shadow: bool,
    /// Answers as JSON.
    pub answers: serde_json::Value,
}

/// Decision spend against the configured caps.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SpendUpdate {
    /// Spend today in US dollars.
    pub today_usd: f64,
    /// Daily cap in US dollars.
    pub daily_cap_usd: f64,
    /// Session cap in US dollars.
    pub session_cap_usd: f64,
}

/// A fact lifecycle change.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FactNotice {
    /// Repository key.
    pub repo: String,
    /// Fact identifier.
    pub fact_id: String,
    /// `confirmed`, `contradicted`, `promoted` or `retired`.
    pub kind: String,
}

/// A redirect or escalation (or the shadow record of one).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RedirectNotice {
    /// Harness of the session.
    pub host: Host,
    /// Host-native session identifier.
    pub session_id: String,
    /// `redirect` or `escalation`.
    pub kind: String,
    /// Text delivered or that would have been delivered.
    pub text: String,
    /// True when nothing was injected.
    pub shadow: bool,
}

/// Typed message of the monitor push stream.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
#[non_exhaustive]
pub enum MonitorMessage {
    /// First frame of every stream.
    Hello {
        /// Wire protocol version, [`crate::PROTOCOL_VERSION`].
        version: u16,
        /// Daemon package version, absent for older protocol-1 peers.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        daemon_version: Option<String>,
        /// Daemon start time in Unix milliseconds, absent for older peers.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        started_at_ms: Option<i64>,
    },
    /// Session state change.
    Session(SessionUpdate),
    /// A decision finished.
    Decision(DecisionNotice),
    /// Spend changed.
    Spend(SpendUpdate),
    /// A fact changed state.
    Fact(FactNotice),
    /// A redirect or escalation happened.
    Redirect(RedirectNotice),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hello_round_trips_with_tag() {
        let json = serde_json::to_string(&MonitorMessage::Hello {
            version: 1,
            daemon_version: None,
            started_at_ms: None,
        })
        .unwrap();
        assert_eq!(json, r#"{"type":"hello","version":1}"#);
        let back: MonitorMessage = serde_json::from_str(&json).unwrap();
        assert_eq!(
            back,
            MonitorMessage::Hello {
                version: 1,
                daemon_version: None,
                started_at_ms: None
            }
        );
    }

    #[test]
    fn hello_metadata_round_trips_and_legacy_frames_decode() {
        let legacy: MonitorMessage =
            serde_json::from_str(r#"{"type":"hello","version":1}"#).unwrap();
        assert!(matches!(
            legacy,
            MonitorMessage::Hello {
                version: 1,
                daemon_version: None,
                started_at_ms: None
            }
        ));
        let message = MonitorMessage::Hello {
            version: 1,
            daemon_version: Some("0.2.0".into()),
            started_at_ms: Some(42),
        };
        let back: MonitorMessage =
            serde_json::from_str(&serde_json::to_string(&message).unwrap()).unwrap();
        assert_eq!(message, back);
    }

    #[test]
    fn redirect_round_trips_through_frame() {
        let message = MonitorMessage::Redirect(RedirectNotice {
            host: Host::Claude,
            session_id: "s".into(),
            kind: "redirect".into(),
            text: "t".into(),
            shadow: true,
        });
        let frame = crate::encode_frame(&message, MONITOR_MAX_FRAME).unwrap();
        let back: MonitorMessage = crate::read_frame(frame.as_slice(), MONITOR_MAX_FRAME).unwrap();
        assert_eq!(back, message);
    }
}
