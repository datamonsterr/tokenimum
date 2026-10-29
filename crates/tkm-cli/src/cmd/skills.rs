//! Skill evidence command entry point.

use std::process::ExitCode;

use crate::{cli::SkillsArgs, ctx::Ctx, error::CliError};

/// Execute `tkm skills`; `--json` emits an empty ranking prior in the shape
/// `tkm_integrations::otherskill::ranking_prior` produces (epic E9 fills it).
///
/// # Errors
/// Returns [`CliError::NotAvailable`] for text output, or an output error if
/// the JSON placeholder cannot be written.
pub fn run(ctx: &mut Ctx<'_>, args: &SkillsArgs) -> Result<ExitCode, CliError> {
    if args.json {
        writeln!(
            ctx.stdout,
            r#"{{"version":1,"source":"tokenimum","skills":[]}}"#
        )?;
        Ok(ExitCode::SUCCESS)
    } else {
        Err(CliError::NotAvailable("skills"))
    }
}
