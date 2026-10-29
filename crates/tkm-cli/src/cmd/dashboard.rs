//! Local web dashboard command entry point.

use std::process::ExitCode;

use crate::{cli::DashboardArgs, ctx::Ctx, error::CliError};

/// Execute `tkm dashboard`.
///
/// # Errors
/// Returns [`CliError::NotAvailable`] until this command is implemented.
pub fn run(_ctx: &mut Ctx<'_>, _args: &DashboardArgs) -> Result<ExitCode, CliError> {
    Err(CliError::NotAvailable("dashboard"))
}
