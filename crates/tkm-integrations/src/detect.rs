//! Adapter detection and runtime health.
use crate::process;
use std::{
    collections::BTreeMap,
    ffi::OsString,
    path::PathBuf,
    str::FromStr,
    sync::{Arc, Mutex},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

/// Configuration override.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Mode {
    /// Enable only when detected.
    #[default]
    Auto,
    /// Enable when the binary exists.
    On,
    /// Never enable.
    Off,
}
impl FromStr for Mode {
    type Err = &'static str;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "auto" => Ok(Self::Auto),
            "on" => Ok(Self::On),
            "off" => Ok(Self::Off),
            _ => Err("invalid integration mode"),
        }
    }
}
/// Current adapter state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AdapterState {
    /// Available.
    Enabled,
    /// Unavailable.
    Disabled {
        /// Explanation.
        reason: String,
    },
    /// Temporarily disabled.
    Backoff {
        /// Clock time at which backoff expires.
        until: Duration,
        /// Explanation.
        reason: String,
    },
}
/// Displayable detection result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdapterStatus {
    /// Adapter name.
    pub name: String,
    /// Configured mode.
    pub mode: Mode,
    /// Current state.
    pub state: AdapterState,
    /// Displayable explanation, if any.
    pub reason: Option<String>,
}
/// Injectable clock for periodic work.
pub trait Clock: Send + Sync {
    /// Current clock value.
    fn now(&self) -> Duration;
}
/// Wall clock implementation.
pub struct SystemClock;
impl Clock for SystemClock {
    fn now(&self) -> Duration {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
    }
}
/// Shared runtime backoff and redetection schedule.
pub struct AdapterHealth {
    clock: Arc<dyn Clock>,
    inner: Mutex<(Option<(Duration, String)>, Option<Duration>)>,
}
impl AdapterHealth {
    /// Creates a health tracker.
    pub fn new(clock: Arc<dyn Clock>) -> Self {
        Self {
            clock,
            inner: Mutex::new((None, None)),
        }
    }
    /// Starts a fifteen-minute backoff.
    pub fn report_failure(&self, reason: &str) {
        if let Ok(mut inner) = self.inner.lock() {
            inner.0 = Some((
                self.clock.now() + Duration::from_secs(900),
                reason.to_owned(),
            ));
        }
    }
    /// Returns whether a failure backoff is active.
    pub fn in_backoff(&self) -> bool {
        self.backoff().is_some()
    }
    /// Returns active backoff state, if any.
    pub fn backoff(&self) -> Option<AdapterState> {
        let inner = self.inner.lock().ok()?;
        let (until, reason) = inner.0.as_ref()?;
        (*until > self.clock.now()).then(|| AdapterState::Backoff {
            until: *until,
            reason: reason.clone(),
        })
    }
    /// Overlays runtime backoff onto a detection result for status reporting.
    pub fn status(&self, mut status: AdapterStatus) -> AdapterStatus {
        if status.mode != Mode::Off {
            if let Some(state @ AdapterState::Backoff { .. }) = self.backoff() {
                if let AdapterState::Backoff { reason, .. } = &state {
                    status.reason = Some(reason.clone());
                }
                status.state = state;
            }
        }
        status
    }
    /// Claims a detection pass at most once per ten minutes.
    pub fn should_redetect(&self) -> bool {
        let Ok(mut inner) = self.inner.lock() else {
            return false;
        };
        let now = self.clock.now();
        if inner
            .1
            .is_some_and(|last| now.saturating_sub(last) < Duration::from_secs(600))
        {
            return false;
        }
        inner.1 = Some(now);
        true
    }
}
/// Probe inputs, isolated from global environment.
pub struct Probe {
    path: OsString,
    home: PathBuf,
    cwd: PathBuf,
    env: BTreeMap<String, String>,
}
impl Probe {
    /// Creates a probe with explicit PATH, home, working directory and environment.
    pub fn new(path: OsString, home: PathBuf, cwd: PathBuf, env: BTreeMap<String, String>) -> Self {
        Self {
            path,
            home,
            cwd,
            env,
        }
    }
    /// Detects a named adapter.
    pub fn detect(&self, name: &str, mode: Mode) -> AdapterStatus {
        let failure = if mode == Mode::Off {
            Some("disabled by configuration")
        } else if let Some(bin) = process::binary(&self.path, name) {
            if mode == Mode::On {
                None
            } else {
                let config = match name {
                    "superorchestrator" => self
                        .env
                        .get("SUPERORCHESTRATOR_CONFIG_DIR")
                        .map(PathBuf::from)
                        .unwrap_or_else(|| {
                            self.env
                                .get("XDG_CONFIG_HOME")
                                .map_or_else(|| self.home.join(".config"), PathBuf::from)
                                .join("superorchestrator")
                        })
                        .is_dir(),
                    "otherskill" => {
                        self.home
                            .join(".config/otherskill/otherskill.config.json")
                            .is_file()
                            || self.cwd.ancestors().any(|p| {
                                p.join("otherskill.config.json").is_file()
                                    || p.join(".otherskill/config.json").is_file()
                            })
                    }
                    _ => false,
                };
                if !config {
                    Some("config missing")
                } else {
                    let mut cmd = std::process::Command::new(bin);
                    cmd.arg(if name == "otherskill" {
                        "--help"
                    } else {
                        "--version"
                    })
                    .env("PATH", &self.path);
                    if process::output(cmd, Duration::from_secs(1)).is_ok() {
                        None
                    } else {
                        Some("version probe failed or timed out")
                    }
                }
            }
        } else {
            Some("binary missing")
        };
        let reason = failure.map(str::to_owned);
        let state = reason
            .as_ref()
            .map_or(AdapterState::Enabled, |s| AdapterState::Disabled {
                reason: s.clone(),
            });
        AdapterStatus {
            name: name.to_owned(),
            mode,
            state,
            reason,
        }
    }
}
