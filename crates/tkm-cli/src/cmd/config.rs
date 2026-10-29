//! Layered configuration command entry point.

use std::process::ExitCode;

use crate::{cli::ConfigArgs, ctx::Ctx, error::CliError};

/// Execute `tkm config`.
///
/// # Errors
/// Returns [`CliError::NotAvailable`] until this command is implemented.
pub fn run(_ctx: &mut Ctx<'_>, _args: &ConfigArgs) -> Result<ExitCode, CliError> {
    Err(CliError::NotAvailable("config"))
}
