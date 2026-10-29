//! Command entry points and top-level dispatch.

use std::process::ExitCode;

use crate::{cli::Command, ctx::Ctx, error::CliError};

mod bench;
pub mod calibrate;
pub mod config;
pub mod dashboard;
pub mod doctor;
pub mod export;
pub mod facts;
pub mod init;
pub mod init_integrations;
pub mod models;
pub mod prune;
pub mod replay;
pub mod run;
pub mod skills;
pub mod status;
pub mod top;
pub mod uninstall;
pub mod why;

/// Dispatch a parsed command with the shared invocation context.
///
/// # Errors
/// Propagates command runtime, usage, or not-available errors.
pub fn dispatch(ctx: &mut Ctx<'_>, command: &Command) -> Result<ExitCode, CliError> {
    match command {
        Command::Init(args) => init::run(ctx, args),
        Command::Uninstall(args) => uninstall::run(ctx, args),
        Command::Status(args) => status::run(ctx, args),
        Command::Top(args) => top::run(ctx, args),
        Command::Dashboard(args) => dashboard::run(ctx, args),
        Command::Why(args) => why::run(ctx, args),
        Command::Facts(args) => facts::run(ctx, args),
        Command::Prune(args) => prune::run(ctx, args),
        Command::Skills(args) => skills::run(ctx, args),
        Command::Config(args) => config::run(ctx, args),
        Command::Models(args) => models::run(ctx, args),
        Command::Calibrate(args) => calibrate::run(ctx, args),
        Command::Replay(args) => replay::run(ctx, args),
        Command::Run(args) => run::run(ctx, args),
        Command::Doctor(args) => doctor::run(ctx, args),
        Command::Export(args) => export::run(ctx, args),
    }
}
