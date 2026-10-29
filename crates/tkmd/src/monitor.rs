//! E4.6 monitor API: a second local socket (`tkmd-monitor.sock`, mode 600) that pushes typed
//! [`MonitorMessage`]s to read-only clients such as `tkm top` (SPEC 11.3).
//!
//! Protocol: connect, read length-prefixed JSON frames (`tkm-proto` framing, limit
//! [`MONITOR_MAX_FRAME`]). The first frame is `Hello`. Nothing the client sends is read as a
//! command. A subscriber that falls more than the broadcast capacity behind, or stops reading
//! for a second, is disconnected; handlers never wait for subscribers.
//!
//! When the daemon is down the socket does not exist. Clients then fall back to the database,
//! opened with [`tkm_store::open_read_only`]: no live feed, but sessions, decisions, facts and
//! the ledger remain readable (WAL mode allows readers beside a writer).

use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use tkm_config::paths::secure_socket;
use tkm_proto::monitor::{MONITOR_MAX_FRAME, MONITOR_SOCKET_NAME, MonitorMessage};
use tkm_proto::{PROTOCOL_VERSION, encode_frame};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::broadcast::error::RecvError;

use crate::daemon::Daemon;

/// Longest a write to one subscriber may take before it is dropped.
const WRITE_LIMIT: Duration = Duration::from_secs(1);

/// A bound monitor listener.
pub struct Monitor {
    listener: UnixListener,
    path: PathBuf,
}

impl Monitor {
    /// Binds `tkmd-monitor.sock` beside the hook socket. The caller already owns the daemon
    /// lock (see [`crate::server::Server::bind`]), so a leftover file is stale.
    #[allow(clippy::unused_async, clippy::unused_async_trait_impl)]
    pub async fn bind(daemon: &Arc<Daemon>) -> io::Result<Self> {
        let paths = daemon.paths();
        paths.ensure_runtime_dir()?;
        let path = paths.socket.with_file_name(MONITOR_SOCKET_NAME);
        match std::fs::remove_file(&path) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
        let listener = UnixListener::bind(&path)?;
        secure_socket(&path)?;
        Ok(Self { listener, path })
    }

    /// Socket path.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Accepts subscribers forever.
    pub async fn run(self, daemon: Arc<Daemon>) {
        loop {
            match self.listener.accept().await {
                Ok((stream, _)) => {
                    // Subscribe before the task starts so no message between accept and the
                    // first recv is lost.
                    let receiver = daemon.subscribe();
                    tokio::spawn(serve_subscriber(stream, receiver, daemon.started_at_ms));
                }
                Err(error) => {
                    daemon.log_error(&format!("monitor accept: {error}"));
                    tokio::time::sleep(Duration::from_millis(100)).await;
                }
            }
        }
    }
}

async fn send(stream: &mut tokio::net::unix::OwnedWriteHalf, message: &MonitorMessage) -> bool {
    let Ok(frame) = encode_frame(message, MONITOR_MAX_FRAME) else {
        return true; // an unencodable message is skipped, the subscriber stays
    };
    matches!(
        tokio::time::timeout(WRITE_LIMIT, stream.write_all(&frame)).await,
        Ok(Ok(()))
    )
}

async fn serve_subscriber(
    stream: UnixStream,
    mut receiver: tokio::sync::broadcast::Receiver<MonitorMessage>,
    started_at_ms: i64,
) {
    let (mut reader, mut writer) = stream.into_split();
    if !send(
        &mut writer,
        &MonitorMessage::Hello {
            version: PROTOCOL_VERSION,
            daemon_version: Some(env!("CARGO_PKG_VERSION").into()),
            started_at_ms: Some(started_at_ms),
        },
    )
    .await
    {
        return;
    }
    let mut scratch = [0_u8; 64];
    loop {
        tokio::select! {
            message = receiver.recv() => match message {
                Ok(message) => {
                    if !send(&mut writer, &message).await {
                        return;
                    }
                }
                // Too slow: drop the subscriber instead of buffering without bound.
                Err(RecvError::Lagged(_) | RecvError::Closed) => return,
            },
            read = reader.read(&mut scratch) => match read {
                Ok(0) | Err(_) => return,
                Ok(_) => {}
            },
        }
    }
}
