//! Optional adapter detection, account homes, consent, and persisted runtime status.

use super::init::state;
use crate::{cli::InitArgs, ctx::Ctx, error::CliError};
use serde::Deserialize;
use std::{
    io::{BufRead, Write},
    path::PathBuf,
};
use tkm_config::config::{EffectiveConfig, LoadInputs};
use tkm_integrations::{
    detect::{AdapterState, Mode, Probe},
    superorchestrator::{self, AccountHome},
};

/// Integration entries displayed by the installation plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IntegrationsPlan {
    /// Superorchestrator detection and enablement summary.
    pub superorchestrator: String,
    /// `otherskill` detection and enablement summary.
    pub otherskill: String,
}

/// Probe optional adapters with E9's bounded, explicit-environment detection.
///
/// # Errors
/// Returns an error if configuration or the home directory cannot be resolved.
pub fn detect(ctx: &Ctx<'_>) -> Result<IntegrationsPlan, CliError> {
    let config = ctx.load_config()?;
    let probe = Probe::new(
        state::env(ctx, "PATH").unwrap_or_default().into(),
        state::home(ctx)?,
        ctx.cwd.clone(),
        ctx.env.iter().cloned().collect(),
    );
    let summary = |name: &str| {
        let mode = config
            .get_str(&format!("integrations.{name}"))
            .and_then(|value| value.parse::<Mode>().ok())
            .unwrap_or_default();
        let status = probe.detect(name, mode);
        if status.state == AdapterState::Enabled {
            format!("{name}: on")
        } else {
            format!(
                "{name}: off ({})",
                status.reason.as_deref().unwrap_or("not detected")
            )
        }
    };
    Ok(IntegrationsPlan {
        superorchestrator: summary("superorchestrator"),
        otherskill: summary("otherskill"),
    })
}

pub(crate) fn account_homes(ctx: &Ctx<'_>, args: &InitArgs) -> Result<Vec<AccountHome>, CliError> {
    if !args.superorchestrator {
        return Ok(Vec::new());
    }
    let directory = state::env(ctx, "SUPERORCHESTRATOR_CONFIG_DIR").map_or_else(
        || {
            state::env(ctx, "XDG_CONFIG_HOME").map_or_else(
                || state::home(ctx).map(|home| home.join(".config/superorchestrator")),
                |path| Ok(PathBuf::from(path).join("superorchestrator")),
            )
        },
        |path| Ok(PathBuf::from(path)),
    )?;
    let directory = ctx.cwd.join(directory);
    let mut homes = superorchestrator::account_homes(&directory);
    homes.retain(|account| {
        account.enabled
            && account.host.is_some_and(|host| {
                (args.only.is_empty() || args.only.contains(&host))
                    && args.host.is_none_or(|selected| selected == host)
            })
    });
    let home = ctx.cwd.join(state::home(ctx)?);
    for account in &mut homes {
        if let Ok(relative) = account.home.strip_prefix("~") {
            account.home = home.join(relative);
        } else if account.home.is_relative() {
            account.home = directory.join(&account.home);
        }
    }
    let mut seen = std::collections::BTreeSet::new();
    homes.retain(|account| {
        seen.insert((account.host.map(|host| host.as_str()), account.home.clone()))
    });
    Ok(homes)
}

pub(crate) fn consent(ctx: &mut Ctx<'_>, yes: bool) -> Result<bool, CliError> {
    // Only the user's file records prior approval; merged repository/env values cannot.
    let config = tkm_config::config::load(&LoadInputs {
        global_path: ctx.paths.global_config.clone(),
        ..LoadInputs::default()
    })?;
    if config.get_bool("integrations.otherskill_key_reuse") == Some(true) {
        return Ok(true);
    }
    if yes
        || !super::init::key::stdin_is_terminal()
        || !tkm_integrations::otherskill::key_path(&state::home(ctx)?).is_file()
    {
        return Ok(false);
    }
    ask_key_reuse(&mut std::io::stdin().lock(), ctx.stderr)
}

fn ask_key_reuse(input: &mut impl BufRead, output: &mut dyn Write) -> Result<bool, CliError> {
    write!(
        output,
        "Allow tokenimum to read otherskill's OpenRouter key in place? [y/N] "
    )?;
    output.flush()?;
    let mut answer = String::new();
    input.read_line(&mut answer)?;
    Ok(matches!(
        answer.trim().to_ascii_lowercase().as_str(),
        "y" | "yes"
    ))
}

#[derive(Deserialize)]
struct StoredStatus {
    name: String,
    mode: String,
    state: String,
    reason: Option<String>,
}

pub(crate) struct AdapterReport {
    pub name: String,
    pub enabled: bool,
    pub reason: String,
}

pub(crate) fn adapter_reports(ctx: &Ctx<'_>, config: &EffectiveConfig) -> Vec<AdapterReport> {
    // A partial file during the daemon's refresh is treated like absent status.
    let stored = std::fs::read(ctx.paths.data_dir.join("integrations/status.json"))
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Vec<StoredStatus>>(&bytes).ok())
        .unwrap_or_default();
    ["superorchestrator", "otherskill"]
        .into_iter()
        .map(|name| {
            let mode = config
                .get_str(&format!("integrations.{name}"))
                .unwrap_or("auto");
            let status = stored.iter().find(|row| {
                row.name == name
                    && matches!(row.mode.as_str(), "auto" | "on" | "off")
                    && matches!(row.state.as_str(), "enabled" | "disabled" | "backoff")
            });
            let (enabled, reason) = if mode == "off" {
                (false, "disabled in configuration".into())
            } else if let Some(row) = status {
                let detail = match row.state.as_str() {
                    "enabled" => "enabled".into(),
                    "backoff" => format!(
                        "backoff: {}",
                        row.reason.as_deref().unwrap_or("temporarily disabled")
                    ),
                    _ => format!(
                        "disabled: {}",
                        row.reason.as_deref().unwrap_or("not detected")
                    ),
                };
                (
                    row.state == "enabled",
                    format!("{detail}; configured {}", row.mode),
                )
            } else {
                (false, format!("adapter unavailable; configured {mode}"))
            };
            AdapterReport {
                name: name.into(),
                enabled,
                reason: super::status::report::line(&reason),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::pedantic)]
    use super::*;

    #[test]
    fn otherskill_key_consent_requires_an_explicit_yes() {
        for (answer, expected) in [
            ("\n", false),
            ("no\n", false),
            ("yes\n", true),
            ("y\n", true),
        ] {
            let mut output = Vec::new();
            assert_eq!(
                ask_key_reuse(&mut answer.as_bytes(), &mut output).unwrap(),
                expected
            );
            let prompt = String::from_utf8(output).unwrap();
            assert!(prompt.contains("otherskill"));
            assert!(prompt.contains("[y/N]"));
        }
    }
}
