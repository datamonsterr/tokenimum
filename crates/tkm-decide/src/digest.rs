//! Structured decision digest.
use crate::redact::Redactor;
use tkm_core::model::Call;
/// Maximum digest length in bytes.
pub const MAX_DIGEST_BYTES: usize = 16384;
/// Test results included in a digest.
#[derive(Debug, Clone, Default)]
pub struct TestSummary {
    /// Passed tests.
    pub passed: usize,
    /// Failed tests.
    pub failed: usize,
}
/// Source material for a digest.
pub struct DigestInput<'a> {
    /// User prompt.
    pub prompt: &'a str,
    /// Calls in chronological order.
    pub calls: &'a [Call],
    /// Optional test results.
    pub tests: Option<TestSummary>,
}
/// Builds a bounded, redacted digest.
#[must_use]
pub fn build_digest(_input: DigestInput<'_>, _redactor: &Redactor) -> String {
    String::new()
}
