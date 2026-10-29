//! Terminal monitor command entry point.

use std::process::ExitCode;

use crate::{cli::TopArgs, ctx::Ctx, error::CliError};

/// Execute `tkm top`.
///
/// # Errors
/// Returns [`CliError::NotAvailable`] until this command is implemented.
pub fn run(_ctx: &mut Ctx<'_>, _args: &TopArgs) -> Result<ExitCode, CliError> {
    Err(CliError::NotAvailable("top"))
}
