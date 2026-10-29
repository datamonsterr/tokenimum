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

use crate::daemon::{Daemon, noop_reply};

/// Longest a client may take to deliver a header or body.
const READ_LIMIT: Duration = Duration::from_secs(5);
/// How long a probe waits for a live daemon to accept.
const PROBE_LIMIT: Duration = Duration::from_millis(500);

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
    /// connect probe fails: a live daemon's socket is never taken over.
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
            if matches!(probe, Ok(Ok(_))) {
                return Ok(Bind::AlreadyRunning);
            }
            std::fs::remove_file(&paths.socket)?;
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
        loop {
            tokio::select! {
                accepted = self.listener.accept() => {
                    match accepted {
                        Ok((stream, _)) => {
                            let daemon = self.daemon.clone();
                            tokio::spawn(async move { serve_connection(stream, &daemon).await });
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

async fn serve_connection(mut stream: UnixStream, daemon: &Arc<Daemon>) {
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
