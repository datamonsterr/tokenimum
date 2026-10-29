//! Layered configuration command entry point.
use crate::{
    cli::{ConfigArgs, ConfigCommand, ConfigLayerArgs},
    ctx::Ctx,
    error::CliError,
};
use std::{
    path::PathBuf,
    process::{Command, ExitCode},
};
use tkm_config::config::{self, ConfigFile, Layer, LoadInputs, Value};

/// Execute configuration operations.
///
/// # Errors
/// Returns an error for invalid configuration, editor failure, or I/O failure.
pub fn run(ctx: &mut Ctx<'_>, args: &ConfigArgs) -> Result<ExitCode, CliError> {
    match &args.command {
        ConfigCommand::Set(args) => {
            let (path, layer) = target(ctx, &args.layer)?;
            let mut file = ConfigFile::load(path, layer)?;
            file.set(&args.key, &args.value)?;
            file.save()?;
        }
        ConfigCommand::Unset(args) => {
            let (path, layer) = target(ctx, &args.layer)?;
            let mut file = ConfigFile::load(path, layer)?;
            file.unset(&args.key)?;
            file.save()?;
        }
        ConfigCommand::Preset(args) => {
            let (path, layer) = if args.repo {
                repo_target(ctx, false)?
            } else {
                (ctx.paths.global_config.clone(), Layer::Global)
            };
            let mut file = ConfigFile::load(path, layer)?;
            file.set("preset", &args.name)?;
            file.save()?;
        }
        ConfigCommand::Edit(args) => {
            let (path, layer) = if args.repo {
                repo_target(ctx, false)?
            } else {
                (ctx.paths.global_config.clone(), Layer::Global)
            };
            edit(ctx, path, layer)?;
        }
        command => {
            let config = ctx.load_config()?;
            for ignored in &config.ignored {
                writeln!(
                    ctx.stderr,
                    "ignored {} ({:?}): {}",
                    ignored.key, ignored.layer, ignored.reason
                )?;
            }
            for error in &config.errors {
                writeln!(ctx.stderr, "ignored invalid configuration: {error}")?;
            }
            match command {
                ConfigCommand::Show(args) => {
                    for (key, value, layer) in config.entries() {
                        if args.effective {
                            writeln!(ctx.stdout, "{key} = {} ({layer:?})", display(value))?;
                        } else {
                            writeln!(ctx.stdout, "{key} = {}", display(value))?;
                        }
                    }
                }
                ConfigCommand::Get(args) => {
                    let value = config
                        .get(&args.key)
                        .ok_or_else(|| CliError::Usage(format!("unknown key: {}", args.key)))?;
                    writeln!(ctx.stdout, "{}", display(value))?;
                }
                ConfigCommand::Explain(args) => {
                    let text = config::explain(&config, &args.key)
                        .ok_or_else(|| CliError::Usage(format!("unknown key: {}", args.key)))?;
                    writeln!(ctx.stdout, "{text}")?;
                }
                ConfigCommand::Diff(_) => {
                    for key in config::diff_from_preset(&config) {
                        if let Some(value) = config.get(&key) {
                            writeln!(ctx.stdout, "{key} = {}", display(value))?;
                        }
                    }
                }
                _ => {
                    return Err(CliError::Usage(
                        "unsupported configuration operation".into(),
                    ));
                }
            }
        }
    }
    Ok(ExitCode::SUCCESS)
}
fn display(value: &Value) -> String {
    match value {
        Value::Bool(v) => v.to_string(),
        Value::Int(v) => v.to_string(),
        Value::Float(v) => v.to_string(),
        Value::String(v) => v.clone(),
        Value::List(v) => format!("{v:?}"),
        Value::Unset => "<unset>".into(),
    }
}
fn repo_target(ctx: &Ctx<'_>, local: bool) -> Result<(PathBuf, Layer), CliError> {
    if ctx.repo.how == tkm_config::repo::IdentitySource::Dir {
        return Err(CliError::Usage(
            "repository configuration requires a repository".into(),
        ));
    }
    Ok(if local {
        (
            ctx.repo.root.join(".tokenimum.local.toml"),
            Layer::RepoLocal,
        )
    } else {
        (ctx.repo.root.join(".tokenimum.toml"), Layer::Repo)
    })
}
fn target(ctx: &Ctx<'_>, args: &ConfigLayerArgs) -> Result<(PathBuf, Layer), CliError> {
    if args.repo {
        repo_target(ctx, false)
    } else if args.local {
        repo_target(ctx, true)
    } else {
        Ok((ctx.paths.global_config.clone(), Layer::Global))
    }
}
fn edit(ctx: &Ctx<'_>, path: PathBuf, layer: Layer) -> Result<(), CliError> {
    let parent = path
        .parent()
        .ok_or_else(|| CliError::Usage("configuration has no parent directory".into()))?;
    std::fs::create_dir_all(parent)?;
    let temp = parent.join(format!(".tokenimum-edit-{}", std::process::id()));
    std::fs::create_dir(&temp)?;
    let staged = temp.join(if layer == Layer::Repo {
        ".tokenimum.toml"
    } else {
        "config.toml"
    });
    let result = (|| {
        match std::fs::read(&path) {
            Ok(bytes) => std::fs::write(&staged, bytes)?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => std::fs::write(&staged, "")?,
            Err(e) => return Err(e.into()),
        }
        let editor = ctx
            .env
            .iter()
            .find(|(key, _)| key == "EDITOR")
            .map(|(_, v)| v.as_str())
            .filter(|v| !v.is_empty())
            .ok_or_else(|| CliError::Usage("EDITOR is not set".into()))?;
        let mut parts = editor.split_whitespace();
        let program = parts
            .next()
            .ok_or_else(|| CliError::Usage("EDITOR is empty".into()))?;
        if !Command::new(program)
            .args(parts)
            .arg(&staged)
            .status()?
            .success()
        {
            return Err(CliError::Usage("editor failed".into()));
        }
        let inputs = LoadInputs {
            global_path: if layer == Layer::Global {
                staged.clone()
            } else {
                temp.join("missing")
            },
            repo_root: (layer == Layer::Repo).then(|| temp.clone()),
            ..LoadInputs::default()
        };
        let checked = config::load(&inputs)?;
        if let Some(error) = checked.errors.first() {
            return Err(CliError::Usage(format!(
                "invalid edited configuration: {error}"
            )));
        }
        if let Some(ignored) = checked.ignored.first() {
            return Err(CliError::Usage(format!(
                "repository cannot set {}",
                ignored.key
            )));
        }
        std::fs::rename(&staged, &path)?;
        Ok(())
    })();
    let _ = std::fs::remove_dir_all(temp);
    result
}
