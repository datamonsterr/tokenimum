//! Recorded session replay command entry point.

use std::process::ExitCode;

use crate::{cli::ReplayArgs, ctx::Ctx, error::CliError};

/// Execute `tkm replay`.
///
/// # Errors
/// Returns [`CliError::NotAvailable`] until this command is implemented.
pub fn run(_ctx: &mut Ctx<'_>, _args: &ReplayArgs) -> Result<ExitCode, CliError> {
    Err(CliError::NotAvailable("replay"))
}
