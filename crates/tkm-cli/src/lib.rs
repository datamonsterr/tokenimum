#![forbid(unsafe_code)]
#![deny(missing_docs)]
// CLI types deliberately include their domain in their names for downstream command modules.
#![allow(clippy::module_name_repetitions)]
//! The `tkm` command tree, shared context, and daemon/database access.
//!
//! Commands receive explicit writers through [`ctx::Ctx`] and return typed errors.
//! Read commands use [`daemon::open_db`] even while the daemon is running; mutating
//! commands must explicitly choose [`daemon::open_db_for_write`].

pub mod cli;
pub mod cmd;
pub mod ctx;
pub mod daemon;
pub mod error;
