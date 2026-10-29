//! Installation diagnostics command entry point.

use std::process::ExitCode;

use crate::{cli::DoctorArgs, ctx::Ctx, error::CliError};

/// Execute `tkm doctor`.
///
/// # Errors
/// Returns [`CliError::NotAvailable`] until this command is implemented.
pub fn run(_ctx: &mut Ctx<'_>, _args: &DoctorArgs) -> Result<ExitCode, CliError> {
    Err(CliError::NotAvailable("doctor"))
}
