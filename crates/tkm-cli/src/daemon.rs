//! Bounded daemon monitoring and explicit read-only/database-write access.

use tkm_config::paths::Paths;

use crate::error::CliError;

#[cfg(unix)]
use std::path::Path;
#[cfg(unix)]
use std::time::Duration;
#[cfg(unix)]
use tkm_proto::monitor::{MONITOR_MAX_FRAME, MONITOR_SOCKET_NAME, MonitorMessage};
#[cfg(unix)]
use tkm_proto::{PROTOCOL_VERSION, decode_body, decode_len};
#[cfg(unix)]
use tokio::io::AsyncReadExt;

/// Maximum duration of the whole connect-and-Hello exchange.
#[cfg(unix)]
const PROBE_TIMEOUT: Duration = Duration::from_millis(250);

/// Availability reported by the daemon's read-only monitor socket.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DaemonState {
    /// A compatible monitor sent its Hello frame.
    Running {
        /// Wire protocol version advertised by Hello, not the daemon package version.
        version: u16,
    },
    /// The monitor cannot supply live data; read commands can still open the database.
    Down {
        /// Human-readable connection, timeout, or protocol failure.
        reason: String,
    },
}

/// Connection returned by [`open_db`], opened with SQLite's read-only flags.
pub type Db = rusqlite::Connection;

/// Probe the monitor socket beside `paths.socket`, without starting the daemon.
///
/// A single 250 ms deadline covers connecting, reading the header, and reading
/// the body. Lengths are checked before allocation and incompatible versions
/// fail open to [`DaemonState::Down`]. A joined worker owns the small runtime so
/// synchronous callers can also use this helper from inside an existing runtime.
#[must_use]
#[cfg(unix)]
pub fn probe(paths: &Paths) -> DaemonState {
    let socket = paths.socket.with_file_name(MONITOR_SOCKET_NAME);
    // A separate thread avoids nested-block_on and runtime-drop panics in async callers.
    match std::thread::Builder::new()
        .name("tkm-monitor-probe".into())
        .spawn(move || probe_socket(&socket))
    {
        Ok(worker) => worker.join().unwrap_or_else(|_| DaemonState::Down {
            reason: "monitor probe worker failed".into(),
        }),
        Err(error) => DaemonState::Down {
            reason: format!("monitor probe worker: {error}"),
        },
    }
}

#[cfg(unix)]
fn probe_socket(socket: &Path) -> DaemonState {
    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_io()
        .enable_time()
        .build()
    {
        Ok(runtime) => runtime,
        Err(error) => {
            return DaemonState::Down {
                reason: format!("monitor runtime: {error}"),
            };
        }
    };
    let outcome =
        runtime.block_on(async { tokio::time::timeout(PROBE_TIMEOUT, read_hello(socket)).await });
    match outcome {
        Ok(Ok(version)) => DaemonState::Running { version },
        Ok(Err(reason)) => DaemonState::Down { reason },
        Err(_) => DaemonState::Down {
            reason: "monitor connect/Hello timed out after 250 ms".into(),
        },
    }
}

/// Report monitor unavailability on platforms without Unix-domain sockets.
#[must_use]
#[cfg(not(unix))]
pub fn probe(_paths: &Paths) -> DaemonState {
    DaemonState::Down {
        reason: "monitor sockets require Unix-domain socket support".into(),
    }
}

#[cfg(unix)]
async fn read_hello(socket: &Path) -> Result<u16, String> {
    let mut stream = tokio::net::UnixStream::connect(socket)
        .await
        .map_err(|error| format!("monitor {}: {error}", socket.display()))?;
    let mut header = [0_u8; 4];
    stream
        .read_exact(&mut header)
        .await
        .map_err(|error| format!("monitor Hello header: {error}"))?;
    let length = decode_len(header, MONITOR_MAX_FRAME)
        .map_err(|error| format!("monitor Hello header: {error}"))?;
    let mut body = vec![0_u8; length];
    stream
        .read_exact(&mut body)
        .await
        .map_err(|error| format!("monitor Hello body: {error}"))?;
    match decode_body::<MonitorMessage>(&body)
        .map_err(|error| format!("monitor Hello body: {error}"))?
    {
        MonitorMessage::Hello { version } if version == PROTOCOL_VERSION => Ok(version),
        MonitorMessage::Hello { version } => Err(format!(
            "monitor protocol version {version} is incompatible with {PROTOCOL_VERSION}"
        )),
        _ => Err("monitor's first frame was not Hello".into()),
    }
}

/// Open the existing database read-only, whether the daemon is up or down.
///
/// This never creates a database, runs migrations, or creates directories.
///
/// # Errors
/// Returns a clear error when the database is absent or cannot be opened.
pub fn open_db(paths: &Paths) -> Result<Db, CliError> {
    if !paths.db.try_exists()? {
        return Err(CliError::MissingDatabase(paths.db.clone()));
    }
    tkm_store::open_read_only(&paths.db).map_err(|source| CliError::Database {
        path: paths.db.clone(),
        source,
    })
}

/// Open or initialize a writable store for an explicitly mutating command.
///
/// Read commands must use [`open_db`] instead. This operation may create
/// directories, create the database, and apply migrations.
///
/// # Errors
/// Returns an error if the store cannot be created, opened, or migrated.
pub fn open_db_for_write(paths: &Paths) -> Result<tkm_store::Store, CliError> {
    tkm_store::Store::open(&paths.db).map_err(|source| CliError::Database {
        path: paths.db.clone(),
        source,
    })
}

#[cfg(all(test, unix))]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use std::io::{Read, Write};
    use std::os::unix::net::UnixListener;
    use std::sync::mpsc;
    use std::time::Duration;

    use tkm_proto::encode_frame;

    use super::*;

    fn paths(home: &tempfile::TempDir) -> Paths {
        // A short path also stays within macOS's Unix socket path limit.
        let mut paths = Paths::from_dirs(home.path(), None, None, None);
        paths.socket = home.path().join("tkmd.sock");
        paths
    }

    #[test]
    fn probe_missing_socket_is_down() {
        let home = tempfile::tempdir().unwrap();
        assert!(matches!(probe(&paths(&home)), DaemonState::Down { reason } if !reason.is_empty()));
    }

    #[test]
    fn probe_reads_monitor_hello() {
        let home = tempfile::tempdir().unwrap();
        let paths = paths(&home);
        let listener =
            UnixListener::bind(paths.socket.with_file_name(MONITOR_SOCKET_NAME)).unwrap();
        let peer = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let frame = encode_frame(
                &MonitorMessage::Hello {
                    version: PROTOCOL_VERSION,
                },
                MONITOR_MAX_FRAME,
            )
            .unwrap();
            stream.write_all(&frame).unwrap();
        });
        let state = probe(&paths);
        peer.join().unwrap();
        assert_eq!(
            state,
            DaemonState::Running {
                version: PROTOCOL_VERSION
            }
        );
    }

    #[test]
    fn probe_rejects_incompatible_protocol() {
        assert_rejected_frame(
            &encode_frame(
                &MonitorMessage::Hello {
                    version: PROTOCOL_VERSION + 1,
                },
                MONITOR_MAX_FRAME,
            )
            .unwrap(),
        );
    }

    #[test]
    fn probe_rejects_oversize_header() {
        assert_rejected_frame(&u32::try_from(MONITOR_MAX_FRAME + 1).unwrap().to_be_bytes());
    }

    #[test]
    fn probe_rejects_malformed_body() {
        assert_rejected_frame(b"\0\0\0\x01{");
    }

    #[test]
    fn probe_rejects_truncated_frame() {
        assert_rejected_frame(b"\0\0");
    }

    #[test]
    fn probe_rejects_non_hello_frame() {
        let message = MonitorMessage::Spend(tkm_proto::monitor::SpendUpdate {
            today_usd: 0.0,
            daily_cap_usd: 1.0,
            session_cap_usd: 0.1,
        });
        assert_rejected_frame(&encode_frame(&message, MONITOR_MAX_FRAME).unwrap());
    }

    fn assert_rejected_frame(frame: &[u8]) {
        let home = tempfile::tempdir().unwrap();
        let paths = paths(&home);
        let listener =
            UnixListener::bind(paths.socket.with_file_name(MONITOR_SOCKET_NAME)).unwrap();
        let frame = frame.to_vec();
        let peer = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream.write_all(&frame).unwrap();
        });
        let state = probe(&paths);
        peer.join().unwrap();
        assert!(matches!(state, DaemonState::Down { reason } if !reason.is_empty()));
    }

    #[test]
    fn probe_times_out_waiting_for_hello() {
        let home = tempfile::tempdir().unwrap();
        let paths = paths(&home);
        let listener =
            UnixListener::bind(paths.socket.with_file_name(MONITOR_SOCKET_NAME)).unwrap();
        let (release, wait) = mpsc::channel();
        let peer = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            wait.recv_timeout(Duration::from_secs(5)).unwrap();
            // The probe must have closed its side when its deadline elapsed.
            let mut byte = [0];
            stream
                .set_read_timeout(Some(Duration::from_secs(1)))
                .unwrap();
            assert_eq!(stream.read(&mut byte).unwrap(), 0);
        });
        let state = probe(&paths);
        release.send(()).unwrap();
        peer.join().unwrap();
        assert!(matches!(state, DaemonState::Down { reason } if reason.contains("timed out")));
    }

    #[test]
    fn open_db_missing_file_is_an_error_without_creation() {
        let home = tempfile::tempdir().unwrap();
        let paths = paths(&home);
        let Err(error) = open_db(&paths) else {
            panic!("missing database must fail");
        };
        assert!(error.to_string().contains(&paths.db.display().to_string()));
        assert_eq!(error.exit_code(), std::process::ExitCode::FAILURE);
        assert!(!paths.data_dir.exists());
    }

    #[test]
    fn open_db_is_read_only_beside_writer() {
        let home = tempfile::tempdir().unwrap();
        let paths = paths(&home);
        let _writer = open_db_for_write(&paths).unwrap();
        let reader = open_db(&paths).unwrap();
        assert!(
            reader
                .execute_batch("CREATE TABLE forbidden (id INTEGER)")
                .is_err()
        );
        let version: i64 = reader
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap();
        assert!(version > 0);
    }
}
