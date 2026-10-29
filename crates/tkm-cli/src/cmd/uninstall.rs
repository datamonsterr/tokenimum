//! Uninstallation command entry point.

use std::process::ExitCode;

use crate::{cli::UninstallArgs, ctx::Ctx, error::CliError};

/// Execute `tkm uninstall`.
///
/// # Errors
/// Returns [`CliError::NotAvailable`] until this command is implemented.
pub fn run(_ctx: &mut Ctx<'_>, _args: &UninstallArgs) -> Result<ExitCode, CliError> {
    Err(CliError::NotAvailable("uninstall"))
}
