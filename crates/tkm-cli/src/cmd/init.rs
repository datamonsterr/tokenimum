//! Installation command entry point.

use std::process::ExitCode;

use crate::{cli::InitArgs, ctx::Ctx, error::CliError};

/// Execute `tkm init`.
///
/// # Errors
/// Returns [`CliError::NotAvailable`] until this command is implemented.
pub fn run(_ctx: &mut Ctx<'_>, _args: &InitArgs) -> Result<ExitCode, CliError> {
    Err(CliError::NotAvailable("init"))
}
