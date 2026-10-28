#![forbid(unsafe_code)]
#![deny(missing_docs)]
//! Layered configuration and presets.

#[cfg(test)]
mod tests {
    #[test]
    fn arithmetic_works() {
        assert_eq!(2 + 2, 4);
    }
}

/// Deliberate CI gate violation: `unwrap` in library code (reverted).
#[must_use]
pub fn gate_violation() -> u8 {
    "1".parse::<u8>().unwrap()
}
