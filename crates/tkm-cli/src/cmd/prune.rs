//! Tool deny-list management command entry point.

use std::process::ExitCode;

use crate::{cli::PruneArgs, ctx::Ctx, error::CliError};

/// Execute `tkm prune`.
///
/// # Errors
/// Returns [`CliError::NotAvailable`] until this command is implemented.
pub fn run(_ctx: &mut Ctx<'_>, _args: &PruneArgs) -> Result<ExitCode, CliError> {
    Err(CliError::NotAvailable("prune"))
}
