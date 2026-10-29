//! Repository status command entry point.

use std::process::ExitCode;

use crate::{cli::StatusArgs, ctx::Ctx, error::CliError};

/// Execute `tkm status`.
///
/// # Errors
/// Returns [`CliError::NotAvailable`] until this command is implemented.
pub fn run(_ctx: &mut Ctx<'_>, _args: &StatusArgs) -> Result<ExitCode, CliError> {
    Err(CliError::NotAvailable("status"))
}
