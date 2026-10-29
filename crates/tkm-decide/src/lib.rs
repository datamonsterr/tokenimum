#![forbid(unsafe_code)]
#![deny(missing_docs)]
//! `OpenRouter` decision client and calibration (SPEC section 8).
//!
//! The crate does no persistence and reads no configuration files: E4 wires
//! storage and configuration through the traits and structs defined here.

pub mod api;
pub mod budget;
pub mod cache;
pub mod calibrate;
pub mod chain;
pub mod client;
pub mod config;
pub mod digest;
pub mod key;
pub mod pricing;
pub mod redact;
