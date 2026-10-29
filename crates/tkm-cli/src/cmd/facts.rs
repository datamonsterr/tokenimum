//! Fact evidence and retirement command entry point.

use std::process::ExitCode;

use crate::{cli::FactsArgs, ctx::Ctx, error::CliError};

/// Execute `tkm facts`.
///
/// # Errors
/// Returns [`CliError::NotAvailable`] until this command is implemented.
pub fn run(_ctx: &mut Ctx<'_>, _args: &FactsArgs) -> Result<ExitCode, CliError> {
    Err(CliError::NotAvailable("facts"))
}
