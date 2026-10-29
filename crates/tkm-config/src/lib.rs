#![forbid(unsafe_code)]
#![deny(missing_docs)]
//! Layered configuration and repository paths.
//!
//! Defaults and a preset are overlaid by global, repository, repository-local,
//! environment and flag values. Repository layers cannot set sensitive keys.
//! Repository identities share a normalized remote or real checkout path;
//! filesystem paths include the database, error log and runtime socket.
//!
//! ```
//! use std::time::{SystemTime, UNIX_EPOCH};
//! use tkm_config::config::{load, LoadInputs};
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let nonce = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
//! let missing = std::env::temp_dir().join(format!("tkm-doc-{}-{nonce}", std::process::id()));
//! let inputs = LoadInputs { global_path: missing, ..LoadInputs::default() };
//! let config = load(&inputs)?;
//! assert_eq!(config.get_int("inject.max_tokens"), Some(600));
//! # Ok(())
//! # }
//! ```

/// Filesystem locations and error log.
pub mod paths;
/// Repository identity resolution.
pub mod repo;

/// Configuration layers, validation, presets and editing.
pub mod config;
