//! Key selection and terminal input without exposing key material.

use std::fs;
use std::io::{IsTerminal, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};

use tkm_config::config::{ConfigFile, EffectiveConfig};
use tkm_decide::key::{ApiKey, KeyError, default_key_file, load_key};

use super::state;
use crate::{ctx::Ctx, error::CliError};

pub(crate) enum Choice {
    Offline,
    Environment,
    Reuse(PathBuf),
    Enter(String),
}

pub(crate) fn key_path(ctx: &Ctx<'_>, config: &EffectiveConfig) -> Result<PathBuf, CliError> {
    let home = state::home(ctx)?;
    let path = config
        .get_str("decide.api_key_path")
        .unwrap_or("~/.config/tokenimum/openrouter.env");
    Ok(path
        .strip_prefix("~/")
        .map_or_else(|| PathBuf::from(path), |relative| home.join(relative)))
}

pub(crate) fn load(ctx: &Ctx<'_>, config: &EffectiveConfig) -> Result<ApiKey, KeyError> {
    let path = key_path(ctx, config).map_err(|_| KeyError::Missing)?;
    load_key(
        state::env(ctx, "OPENROUTER_API_KEY").map(str::to_owned),
        &path,
        None,
    )
}

pub(crate) fn automatic(ctx: &Ctx<'_>, config: &EffectiveConfig) -> Result<Choice, CliError> {
    let path = key_path(ctx, config)?;
    if let Some(value) = state::env(ctx, "OPENROUTER_API_KEY").filter(|v| !v.trim().is_empty()) {
        load_key(Some(value.to_owned()), &path, None).map_err(state::failure)?;
        return Ok(Choice::Environment);
    }
    if path.try_exists()? {
        load_key(None, &path, None).map_err(state::failure)?;
        Ok(Choice::Reuse(path))
    } else {
        Ok(Choice::Offline)
    }
}

pub(crate) fn describe(choice: &Choice) -> String {
    match choice {
        Choice::Offline => "offline (no key file)".into(),
        Choice::Environment => "from environment".into(),
        Choice::Reuse(path) => format!("reuse {}", path.display()),
        Choice::Enter(_) => "new key (hidden)".into(),
    }
}

pub(crate) fn interactive(ctx: &mut Ctx<'_>, choice: Choice) -> Result<Choice, CliError> {
    writeln!(
        ctx.stderr,
        "Key choice: enter 'reuse <path>', 'key', or 'offline'; press Enter for {}.",
        describe(&choice)
    )?;
    ctx.stderr.flush()?;
    let mut input = String::new();
    std::io::stdin().read_line(&mut input)?;
    let input = input.trim();
    if input.is_empty() {
        return Ok(choice);
    }
    if input == "offline" {
        return Ok(Choice::Offline);
    }
    if let Some(path) = input.strip_prefix("reuse ") {
        let home = state::home(ctx)?;
        let path = path.trim();
        let path = path
            .strip_prefix("~/")
            .map_or_else(|| PathBuf::from(path), |relative| home.join(relative));
        load_key(None, &path, None).map_err(state::failure)?;
        return Ok(Choice::Reuse(path));
    }
    if input != "key" {
        return Err(CliError::Usage(
            "choose reuse <path>, key, or offline".into(),
        ));
    }
    let stty = state::executable(ctx, "stty").ok_or_else(|| {
        state::failure("stty is required for hidden key entry; use reuse <path> or offline")
    })?;
    let status = Command::new(&stty).arg("-echo").status()?;
    if !status.success() {
        return Err(state::failure("cannot disable terminal echo"));
    }
    let guard = EchoGuard(stty);
    write!(ctx.stderr, "OpenRouter key (hidden): ")?;
    ctx.stderr.flush()?;
    let mut secret = String::new();
    let read = std::io::stdin().read_line(&mut secret);
    drop(guard);
    writeln!(ctx.stderr)?;
    read?;
    let secret = secret.trim().to_owned();
    if secret.is_empty()
        || secret
            .chars()
            .any(|character| character.is_control() || matches!(character, '\'' | '"' | '#'))
    {
        return Err(state::failure(
            "key was empty or contained unsupported characters",
        ));
    }
    Ok(Choice::Enter(secret))
}

struct EchoGuard(PathBuf);
impl Drop for EchoGuard {
    fn drop(&mut self) {
        let _ = Command::new(&self.0)
            .arg("echo")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
}

pub(crate) fn stdin_is_terminal() -> bool {
    std::io::stdin().is_terminal()
}

pub(crate) fn apply(
    ctx: &Ctx<'_>,
    choice: &Choice,
    config: &mut ConfigFile,
) -> Result<(), CliError> {
    let new_key = if matches!(choice, Choice::Enter(_)) {
        Some(default_key_file(&state::home(ctx)?))
    } else {
        None
    };
    super::reversal::record(ctx, new_key.as_deref())?;
    match choice {
        Choice::Environment => config.set("decide.enabled", "true")?,
        Choice::Offline => config.set("decide.enabled", "false")?,
        Choice::Reuse(path) => {
            config.set(
                "decide.api_key_path",
                path.to_str()
                    .ok_or_else(|| state::failure("key path is not UTF-8"))?,
            )?;
            config.set("decide.enabled", "true")?;
        }
        Choice::Enter(secret) => {
            let path = default_key_file(&state::home(ctx)?);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)?;
            }
            let mut options = fs::OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            let mut file = options.open(&path)?;
            if let Err(error) = super::reversal::created_key(ctx, &path) {
                drop(file);
                let _ = fs::remove_file(&path);
                return Err(error);
            }
            writeln!(file, "OPENROUTER_API_KEY='{secret}'")?;
            file.sync_all()?;
            config.set(
                "decide.api_key_path",
                path.to_str()
                    .ok_or_else(|| state::failure("key path is not UTF-8"))?,
            )?;
            config.set("decide.enabled", "true")?;
        }
    }
    Ok(())
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use tkm_config::{config::Layer, paths::Paths, repo::repo_identity};

    #[test]
    fn uninstall_deletes_only_key_created_by_init() {
        for existing in [false, true] {
            let home = tempfile::tempdir().unwrap();
            let paths = Paths::from_dirs(home.path(), None, None, None);
            let key = default_key_file(home.path());
            if existing {
                fs::create_dir_all(key.parent().unwrap()).unwrap();
                fs::write(&key, "OPENROUTER_API_KEY=fixture-reused\n").unwrap();
            }
            let repo = repo_identity(home.path(), "", Some(home.path()));
            let (mut stdout, mut stderr) = (Vec::new(), Vec::new());
            let mut ctx = Ctx::new(
                paths.clone(),
                home.path().into(),
                repo,
                &mut stdout,
                &mut stderr,
            );
            ctx.env
                .push(("HOME".into(), home.path().display().to_string()));
            daemon_for_test(&ctx);
            let mut global = ConfigFile::load(paths.global_config.clone(), Layer::Global).unwrap();
            let choice = if existing {
                Choice::Reuse(key.clone())
            } else {
                Choice::Enter("fixture-new-secret".into())
            };
            apply(&ctx, &choice, &mut global).unwrap();
            global.save().unwrap();
            assert!(key.is_file());
            let exit = crate::cmd::uninstall::run(
                &mut ctx,
                &crate::cli::UninstallArgs {
                    host: None,
                    dry_run: false,
                },
            )
            .unwrap();
            assert_eq!(exit, std::process::ExitCode::SUCCESS);
            assert_eq!(key.exists(), existing);
            assert!(!paths.global_config.exists());
            if existing {
                assert_eq!(
                    fs::read(&key).unwrap(),
                    b"OPENROUTER_API_KEY=fixture-reused\n"
                );
            }
        }
    }

    fn daemon_for_test(ctx: &Ctx<'_>) {
        crate::daemon::open_db_for_write(&ctx.paths).unwrap();
    }
}
