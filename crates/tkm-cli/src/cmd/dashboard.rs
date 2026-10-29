//! Local web dashboard command entry point.

use std::process::ExitCode;

use crate::{cli::DashboardArgs, ctx::Ctx, error::CliError};

/// Execute `tkm dashboard` and print the daemon's local dashboard URL.
///
/// The web feature is required on `tkmd`; dashboard discovery itself needs no feature.
///
/// # Errors
/// Returns an error if discovery, daemon startup, or output fails.
pub fn run(ctx: &mut Ctx<'_>, args: &DashboardArgs) -> Result<ExitCode, CliError> {
    let url = tkm_ui::cli::run_dashboard(
        &ctx.paths,
        &tkm_ui::cli::DashboardArgs {
            no_open: args.no_open,
        },
    )
    .map_err(super::init::state::failure)?;
    writeln!(ctx.stdout, "{url}")?;
    Ok(ExitCode::SUCCESS)
}
