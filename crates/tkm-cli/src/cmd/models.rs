//! Decision model diagnostics command entry point.

use std::process::ExitCode;

use crate::{cli::ModelsArgs, ctx::Ctx, error::CliError};

/// Execute `tkm models`.
///
/// # Errors
/// Returns [`CliError::NotAvailable`] until this command is implemented.
pub fn run(_ctx: &mut Ctx<'_>, _args: &ModelsArgs) -> Result<ExitCode, CliError> {
    Err(CliError::NotAvailable("models"))
}
