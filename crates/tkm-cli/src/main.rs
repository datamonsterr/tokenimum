#![forbid(unsafe_code)]
//! `tkm` entry point; commands and shared state live in the library.

use std::io::Write;
use std::process::ExitCode;

use clap::Parser;
use tkm_cli::{cli::Cli, cmd, ctx::Ctx};

fn main() -> ExitCode {
    if let Some(code) = cmd::doctor::record_probe_hook() {
        return code;
    }
    let cli = Cli::parse();
    let mut stdout = std::io::stdout().lock();
    let mut stderr = std::io::stderr().lock();
    let result = Ctx::from_env(&mut stdout, &mut stderr)
        .and_then(|mut ctx| cmd::dispatch(&mut ctx, &cli.command));
    match result {
        Ok(code) => code,
        Err(error) => {
            let _ = writeln!(stderr, "{}", error.to_string().replace(['\r', '\n'], " "));
            error.exit_code()
        }
    }
}
