//! Private-corpus replay benchmark support (ADR 0048).
//!
//! Raw records and extracted prompts are kept separate from serializable aggregates.
mod accounting;
mod command;
mod config;
mod report;
mod transcript;
mod types;

pub use accounting::account;
pub use command::{BenchArgs, run};
pub use config::check_config;
pub use report::{Comparison, Measurement, Measurements, Outcome, compare, write_report};
pub use transcript::{Transcript, extract_prompts, load_transcript};
pub use types::{
    Baseline, BenchError, Commit, Manifest, ModelPrice, Prompt, SessionMetrics, SessionSpec,
    Tokens, anchor, parse_git_log,
};
