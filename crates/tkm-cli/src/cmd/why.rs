//! Action explanation command entry point.

use std::process::ExitCode;

use crate::{cli::WhyArgs, ctx::Ctx, error::CliError};

/// Execute `tkm why`.
///
/// # Errors
/// Returns [`CliError::NotAvailable`] until this command is implemented.
pub fn run(_ctx: &mut Ctx<'_>, _args: &WhyArgs) -> Result<ExitCode, CliError> {
    Err(CliError::NotAvailable("why"))
}
