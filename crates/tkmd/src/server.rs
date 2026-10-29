//! E4.2 hook socket server: Unix socket, length-prefixed JSON frames (`tkm-proto`).

use std::fs::File;
use std::io;
use std::sync::Arc;
use std::time::Duration;

use tkm_config::paths::secure_socket;
use tkm_proto::{
    MAX_REPLY_FRAME, MAX_REQUEST_FRAME, Request, decode_body, decode_len, encode_frame,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::oneshot;

use crate::daemon::{Daemon, noop_reply};

/// Longest a client may take to deliver a header or body.
const READ_LIMIT: Duration = Duration::from_secs(5);
/// How long a probe waits for a live daemon to accept.
const PROBE_LIMIT: Duration = Duration::from_millis(500);

fn probe_is_refused(probe: &Result<io::Result<UnixStream>, tokio::time::error::Elapsed>) -> bool {
    matches!(probe, Ok(Err(error)) if error.kind() == io::ErrorKind::ConnectionRefused)
}

/// A bound listener.
pub struct Server {
    listener: UnixListener,
    daemon: Arc<Daemon>,
    // Held for the process lifetime: the advisory lock is what makes a second daemon exit.
    _lock: File,
}

/// Result of [`Server::bind`].
pub enum Bind {
    /// This process owns the socket.
    Bound(Server),
    /// Another daemon is serving; the caller should exit 0.
    AlreadyRunning,
}

impl Server {
    /// Binds the socket from [`tkm_config::paths::Paths`].
    ///
    /// Takes an exclusive advisory lock next to the socket first, so two daemons cannot race to
    /// replace each other's socket. A socket file is removed only after the lock is held and a
    /// connect probe is refused: a live daemon's socket is never taken over.
    pub async fn bind(daemon: Arc<Daemon>) -> io::Result<Bind> {
        let paths = daemon.paths();
        paths.ensure_runtime_dir()?;
        let lock_path = paths.socket.with_file_name("tkmd.lock");
        let lock = File::create(&lock_path)?;
        match lock.try_lock() {
            Ok(()) => {}
            Err(std::fs::TryLockError::WouldBlock) => return Ok(Bind::AlreadyRunning),
            Err(std::fs::TryLockError::Error(error)) => return Err(error),
        }
        if paths.socket.exists() {
            let probe = tokio::time::timeout(PROBE_LIMIT, UnixStream::connect(&paths.socket)).await;
            if probe_is_refused(&probe) {
                std::fs::remove_file(&paths.socket)?;
            } else {
                match probe {
                    Err(_) | Ok(Ok(_)) => return Ok(Bind::AlreadyRunning),
                    Ok(Err(error)) if error.kind() == io::ErrorKind::WouldBlock => {
                        return Ok(Bind::AlreadyRunning);
                    }
                    Ok(Err(error)) => return Err(error),
                }
            }
        }
        let listener = UnixListener::bind(&paths.socket)?;
        secure_socket(&paths.socket)?;
        Ok(Bind::Bound(Self {
            listener,
            daemon,
            _lock: lock,
        }))
    }

    /// Accepts connections until `shutdown` completes.
    pub async fn run(self, shutdown: impl std::future::Future<Output = ()>) -> io::Result<()> {
        tokio::pin!(shutdown);
        let (first, mut last) = oneshot::channel::<()>();
        drop(first);
        loop {
            tokio::select! {
                accepted = self.listener.accept() => {
                    match accepted {
                        Ok((stream, _)) => {
                            let daemon = self.daemon.clone();
                            // Hooks run one after another, so connection order is event order.
                            // Handling follows accept order even when a slow hook gave up early.
                            let (done, next) = oneshot::channel();
                            let previous = std::mem::replace(&mut last, next);
                            tokio::spawn(async move {
                                serve_connection(stream, &daemon, Some(previous), done).await;
                            });
                        }
                        Err(error) => self.daemon.log_error(&format!("accept: {error}")),
                    }
                }
                () = &mut shutdown => break,
            }
        }
        let _ = std::fs::remove_file(&self.daemon.paths().socket);
        Ok(())
    }
}

async fn read_request(stream: &mut UnixStream) -> Result<Option<Request>, String> {
    let mut header = [0_u8; 4];
    match tokio::time::timeout(READ_LIMIT, stream.read_exact(&mut header)).await {
        Err(_) => return Ok(None),
        Ok(Err(error)) if error.kind() == io::ErrorKind::UnexpectedEof => return Ok(None),
        Ok(Err(error)) => return Err(format!("read header: {error}")),
        Ok(Ok(_)) => {}
    }
    // Validate the length before allocating the body.
    let len = decode_len(header, MAX_REQUEST_FRAME)
        .map_err(|error| format!("bad frame header: {error}"))?;
    let mut body = vec![0_u8; len];
    tokio::time::timeout(READ_LIMIT, stream.read_exact(&mut body))
        .await
        .map_err(|_| "read body: timed out".to_owned())?
        .map_err(|error| format!("read body: {error}"))?;
    decode_body::<Request>(&body)
        .map(Some)
        .map_err(|error| format!("bad frame body: {error}"))
}

/// Longest a request waits for the previous connection's request to finish.
const ORDER_LIMIT: Duration = Duration::from_millis(250);

async fn serve_connection(
    mut stream: UnixStream,
    daemon: &Arc<Daemon>,
    mut previous: Option<oneshot::Receiver<()>>,
    done: oneshot::Sender<()>,
) {
    let _done = done; // dropping it releases the next connection
    loop {
        let request = match read_request(&mut stream).await {
            Ok(Some(request)) => request,
            Ok(None) => return,
            Err(message) => {
                // The hook fails open when the connection closes without a reply.
                daemon.log_error(&format!("server: {message}"));
                return;
            }
        };
        // Only the first request of a connection is ordered; hooks send one per connection.
        if let Some(previous) = previous.take() {
            let _ = tokio::time::timeout(ORDER_LIMIT, previous).await;
        }
        let host = request.host;
        let reply = daemon.handle(request).await;
        let frame = encode_frame(&reply, MAX_REPLY_FRAME)
            .or_else(|_| encode_frame(&noop_reply(host), MAX_REPLY_FRAME));
        let Ok(frame) = frame else {
            return;
        };
        if stream.write_all(&frame).await.is_err() {
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn timed_out_probe_never_authorizes_socket_removal() {
        let probe = tokio::time::timeout(Duration::ZERO, std::future::pending::<io::Result<UnixStream>>()).await;
        assert!(!probe_is_refused(&probe));
        let would_block = Ok(Err(io::Error::from(io::ErrorKind::WouldBlock)));
        assert!(!probe_is_refused(&would_block));
        let refused = Ok(Err(io::Error::from(io::ErrorKind::ConnectionRefused)));
        assert!(probe_is_refused(&refused));
    }
}
