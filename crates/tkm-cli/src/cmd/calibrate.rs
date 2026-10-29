//! Decision threshold calibration command entry point.

use std::process::ExitCode;

use crate::{cli::CalibrateArgs, ctx::Ctx, error::CliError};

/// Execute `tkm calibrate`.
///
/// # Errors
/// Returns [`CliError::NotAvailable`] until this command is implemented.
pub fn run(_ctx: &mut Ctx<'_>, _args: &CalibrateArgs) -> Result<ExitCode, CliError> {
    Err(CliError::NotAvailable("calibrate"))
}
