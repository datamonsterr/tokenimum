//! Terminal monitor command entry point.

use std::{io::IsTerminal, process::ExitCode};

use crate::{cli::TopArgs, ctx::Ctx, error::CliError};

/// Execute `tkm top` in an interactive terminal.
///
/// # Errors
/// Returns an error if stdin/stdout are not terminals, or the monitor cannot start.
pub fn run(ctx: &mut Ctx<'_>, _args: &TopArgs) -> Result<ExitCode, CliError> {
    // E8 initializes the terminal with ratatui::init, which panics on redirected stdio.
    if !std::io::stdin().is_terminal() || !std::io::stdout().is_terminal() {
        return Err(super::init::state::failure(
            "tkm top requires an interactive terminal (stdin and stdout)",
        ));
    }
    tkm_ui::cli::run_top(&ctx.paths).map_err(super::init::state::failure)?;
    Ok(ExitCode::SUCCESS)
}
