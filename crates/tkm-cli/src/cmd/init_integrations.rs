//! Optional integration detection placeholder, replaced by epic E9.

use crate::ctx::Ctx;

/// Integration entries displayed by the installation plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IntegrationsPlan {
    /// Superorchestrator detection and enablement summary.
    pub superorchestrator: String,
    /// `otherskill` detection and enablement summary.
    pub otherskill: String,
}

/// Return the initial, disabled integration plan without probing or writing files.
#[must_use]
pub fn detect(_ctx: &Ctx<'_>) -> IntegrationsPlan {
    IntegrationsPlan {
        superorchestrator: "superorchestrator: off (not detected)".into(),
        otherskill: "otherskill: off (not detected)".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn otherskill_key_consent_requires_an_explicit_yes() {
        for (answer, expected) in [
            ("\n", false),
            ("no\n", false),
            ("yes\n", true),
            ("y\n", true),
        ] {
            let mut output = Vec::new();
            assert_eq!(
                ask_key_reuse(&mut answer.as_bytes(), &mut output).unwrap(),
                expected
            );
            let prompt = String::from_utf8(output).unwrap();
            assert!(prompt.contains("otherskill"));
            assert!(prompt.contains("[y/N]"));
        }
    }
}
