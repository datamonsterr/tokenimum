//! Fold-only harness wrapper command entry point.

use std::process::ExitCode;

use crate::{cli::RunArgs, ctx::Ctx, error::CliError};

/// Execute `tkm run`.
///
/// # Errors
/// Returns [`CliError::NotAvailable`] until this command is implemented.
pub fn run(_ctx: &mut Ctx<'_>, _args: &RunArgs) -> Result<ExitCode, CliError> {
    Err(CliError::NotAvailable("run"))
}
