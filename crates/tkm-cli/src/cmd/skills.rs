//! Skill evidence command entry point.

use std::process::ExitCode;

use crate::{cli::SkillsArgs, ctx::Ctx, error::CliError};

/// Execute `tkm skills`, emitting an empty JSON array when requested.
///
/// # Errors
/// Returns [`CliError::NotAvailable`] for text output, or an output error if
/// the JSON placeholder cannot be written.
pub fn run(ctx: &mut Ctx<'_>, args: &SkillsArgs) -> Result<ExitCode, CliError> {
    if args.json {
        writeln!(ctx.stdout, "[]")?;
        Ok(ExitCode::SUCCESS)
    } else {
        Err(CliError::NotAvailable("skills"))
    }
}
