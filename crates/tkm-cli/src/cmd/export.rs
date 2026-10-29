//! Repository state export command entry point.

use std::process::ExitCode;

use crate::{cli::ExportArgs, ctx::Ctx, error::CliError};

/// Execute `tkm export`.
///
/// # Errors
/// Returns [`CliError::NotAvailable`] until this command is implemented.
pub fn run(_ctx: &mut Ctx<'_>, _args: &ExportArgs) -> Result<ExitCode, CliError> {
    Err(CliError::NotAvailable("export"))
}
