//! Background reader for the daemon's monitor socket.
use std::sync::mpsc::{self, Receiver};
use tkm_config::paths::Paths;
use tkm_proto::{
    PROTOCOL_VERSION,
    monitor::{MONITOR_MAX_FRAME, MONITOR_SOCKET_NAME, MonitorMessage},
};

/// Whether a live connection is available.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Connected to the daemon.
    Live,
    /// No live connection.
    ReadOnly,
}
/// Connection status and received messages. Disconnection means the feed ended.
pub struct Feed {
    /// Initial mode.
    pub mode: Mode,
    /// Received messages.
    pub receiver: Receiver<MonitorMessage>,
}
/// Connect and validate the first frame; connection or protocol failure falls back to read-only.
#[must_use]
pub fn connect(paths: &Paths) -> Feed {
    let (sender, receiver) = mpsc::channel();
    #[cfg(unix)]
    {
        use std::{os::unix::net::UnixStream, time::Duration};
        if let Ok(mut stream) =
            UnixStream::connect(paths.socket.with_file_name(MONITOR_SOCKET_NAME))
        {
            let _ = stream.set_read_timeout(Some(Duration::from_secs(1)));
            if matches!(tkm_proto::read_frame::<_, MonitorMessage>(&mut stream, MONITOR_MAX_FRAME), Ok(MonitorMessage::Hello { version }) if version == PROTOCOL_VERSION)
            {
                let _ = stream.set_read_timeout(None);
                std::thread::spawn(move || {
                    while let Ok(message) = tkm_proto::read_frame(&mut stream, MONITOR_MAX_FRAME) {
                        if sender.send(message).is_err() {
                            break;
                        }
                    }
                });
                return Feed {
                    mode: Mode::Live,
                    receiver,
                };
            }
        }
    }
    #[cfg(not(unix))]
    let _ = (paths, sender);
    Feed {
        mode: Mode::ReadOnly,
        receiver,
    }
}
