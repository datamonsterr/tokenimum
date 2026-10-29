#![forbid(unsafe_code)]
#![deny(missing_docs)]
//! Layered configuration and repository paths.

/// Filesystem locations and error log.
pub mod paths;
/// Repository identity resolution.
pub mod repo;

/// Configuration layers, validation, presets and editing.
pub mod config;
