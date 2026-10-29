//! E4.7 diagnostics wiring.
//!
//! Decision: the daemon's own diagnostics (tracing events at `warn` and above, panics) go to
//! `errors.log` through [`tkm_config::paths::ErrorLog`], which rotates at 5 MB. A daemon spawned
//! by the hook has no useful stderr, and one file gives `tkm status` a single place to look.
//! The service units also point stderr at the same file, which catches anything printed before
//! logging starts.

use std::io::{self, Write};
use std::sync::Arc;

use tkm_config::paths::{ErrorLog, Paths};

/// Buffers one tracing record and appends it as one `errors.log` line.
struct LineWriter {
    log: Arc<ErrorLog>,
    buffer: Vec<u8>,
}

impl Write for LineWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.buffer.extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        if self.buffer.is_empty() {
            return Ok(());
        }
        let text = String::from_utf8_lossy(&self.buffer).replace('\n', " ");
        self.buffer.clear();
        self.log.append(text.trim())
    }
}

impl Drop for LineWriter {
    fn drop(&mut self) {
        let _ = self.flush();
    }
}

/// Installs the tracing subscriber and a panic hook that write to `errors.log`.
pub fn init(paths: &Paths) {
    let log = Arc::new(ErrorLog::for_paths(paths));
    let writer_log = log.clone();
    let subscriber = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::WARN)
        .with_ansi(false)
        .without_time()
        .with_writer(move || LineWriter {
            log: writer_log.clone(),
            buffer: Vec::new(),
        })
        .finish();
    let _ = tracing::subscriber::set_global_default(subscriber);
    std::panic::set_hook(Box::new(move |info| {
        let _ = log.append(&format!("panic: {}", info.to_string().replace('\n', " ")));
    }));
}
