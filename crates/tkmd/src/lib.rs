#![forbid(unsafe_code)]
#![deny(missing_docs)]
// Pedantic lints that add noise without value for daemon glue code.
#![allow(
    clippy::missing_errors_doc,
    clippy::missing_panics_doc,
    clippy::must_use_candidate,
    clippy::module_name_repetitions,
    clippy::too_many_lines,
    clippy::significant_drop_tightening,
    clippy::unused_self,
    clippy::needless_pass_by_value,
    clippy::similar_names,
    clippy::cast_possible_truncation
)]
//! The tokenimum daemon (`tkmd`): hook server, live layer, inject layer, fold workers and
//! monitor stream (SPEC 5.1, 6.2, 7.2, 7.4, 11.3, 14).
//!
//! Everything lives in this library so tests and benches drive the same code as the binary.
//! Design records: ADRs 0015 to 0019.
//!
//! Hot-path rule: [`daemon::Daemon::handle_tool_done`] only mutates memory and pushes to
//! in-process queues. Database access belongs to the flusher, the fold workers and the
//! `session_start`/`prompt` read path; network access belongs to the decision worker.

pub mod backing;
pub mod clock;
pub mod daemon;
pub mod decider;
pub mod fold;
pub mod host_level;
pub mod injection;
pub mod live;
pub mod logging;
pub mod monitor;
pub mod persist;
pub mod registry;
pub mod server;
pub mod service;
pub mod settings;
pub mod state;

/// Optional local dashboard.
#[cfg(feature = "web")]
pub mod web;
