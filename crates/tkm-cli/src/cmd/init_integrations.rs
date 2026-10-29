//! Optional integration detection placeholder, replaced by epic E9.

use crate::ctx::Ctx;

/// Integration entries displayed by the installation plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IntegrationsPlan {
    /// Superorchestrator detection and enablement summary.
    pub superorchestrator: String,
    /// OtherSkill detection and enablement summary.
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
