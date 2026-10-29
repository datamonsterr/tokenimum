#![forbid(unsafe_code)]
#![deny(missing_docs)]
//! Terminal monitoring interfaces.
/// Command entry point.
pub mod cli;
/// Monitor socket reader.
pub mod feed;
/// Monitor data and snapshots.
pub mod model;
/// Terminal rendering and interaction.
pub mod tui;
/// Errors returned by the monitor.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// A terminal or filesystem operation failed.
    #[error(transparent)]
    Io(#[from] std::io::Error),
    /// A database operation failed.
    #[error(transparent)]
    Sqlite(#[from] rusqlite::Error),
    /// A store operation failed.
    #[error(transparent)]
    Store(#[from] tkm_store::StoreError),
    /// Stored JSON could not be decoded.
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}

/// Local web dashboard.
#[cfg(feature = "web")]
pub mod web;
