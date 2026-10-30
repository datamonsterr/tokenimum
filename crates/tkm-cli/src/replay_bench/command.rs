use super::{
    Baseline, BenchError, Manifest, Prompt, Transcript, account, anchor, check_config,
    extract_prompts, load_transcript, parse_git_log,
    report::Measurements,
    types::{invalid, validate_commit},
    write_report,
};
use crate::error::CliError;
use clap::{Args, Subcommand, ValueEnum};
use serde::Serialize;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

/// Hidden benchmark command namespace.
#[derive(Debug, Args)]
pub struct BenchArgs {
    #[command(subcommand)]
    command: BenchCommand,
}
#[derive(Debug, Subcommand)]
enum BenchCommand {
    History(HistoryArgs),
}
#[derive(Debug, Args)]
struct HistoryArgs {
    #[command(subcommand)]
    command: HistoryCommand,
}
#[derive(Debug, Subcommand)]
enum HistoryCommand {
    /// Strictly load the bundle and write prompt-free baseline accounting.
    Baseline(CorpusArgs),
    /// Write a private plan with extracted prompts and historical start commits.
    Plan(PlanArgs),
    /// Check two AI configuration snapshots before any live model call.
    ConfigCheck {
        #[arg(long)]
        a: PathBuf,
        #[arg(long)]
        b: PathBuf,
    },
    /// Produce an aggregate-only Markdown artifact from private measurements.
    Report {
        #[arg(long)]
        baseline: PathBuf,
        #[arg(long)]
        measurements: PathBuf,
        #[arg(long)]
        out: PathBuf,
    },
}
#[derive(Debug, Args)]
struct CorpusArgs {
    #[arg(long)]
    manifest: PathBuf,
    #[arg(long)]
    out: PathBuf,
}
#[derive(Debug, Args)]
struct PlanArgs {
    #[command(flatten)]
    corpus: CorpusArgs,
    #[arg(long, value_enum, default_value = "pilot")]
    subset: Subset,
    #[arg(long)]
    confirm_full: bool,
}
#[derive(Debug, Clone, Copy, ValueEnum)]
enum Subset {
    Pilot,
    Sample,
    Full,
}
#[derive(Serialize)]
struct PlannedSession {
    id: String,
    start_commit: Option<String>,
    end_commit: String,
    prompts: Vec<Prompt>,
}
#[derive(Serialize)]
struct Plan {
    replay: Vec<PlannedSession>,
    control: Vec<PlannedSession>,
}

/// Execute a hidden history command. Does not open the tokenimum database.
///
/// # Errors
/// Propagates invalid corpus, configuration and report errors as CLI usage failures.
pub fn run(args: &BenchArgs) -> Result<ExitCode, CliError> {
    let BenchCommand::History(history) = &args.command;
    execute(&history.command).map_err(|error| CliError::Usage(error.to_string()))?;
    Ok(ExitCode::SUCCESS)
}
fn execute(command: &HistoryCommand) -> Result<(), BenchError> {
    match command {
        HistoryCommand::Baseline(args) => {
            let (manifest, transcripts, skipped) = corpus(&args.manifest)?;
            let mut baseline = account(&transcripts, &manifest.prices)?;
            baseline.skipped.extend(skipped);
            save_json(&args.out, &baseline)
        }
        HistoryCommand::Plan(args) => {
            if matches!(args.subset, Subset::Full) && !args.confirm_full {
                return Err(invalid("full replay requires --confirm-full"));
            }
            let (manifest, mut transcripts, _) = corpus(&args.corpus.manifest)?;
            let baseline = account(&transcripts, &manifest.prices)?;
            transcripts.sort_by_key(|t| (t.started_ms, t.spec.id.clone()));
            let eligible: Vec<_> = transcripts
                .iter()
                .filter(|t| {
                    t.intact
                        && t.spec.parent.is_none()
                        && baseline.sessions.contains_key(&t.spec.id)
                })
                .collect();
            let limit = match args.subset {
                Subset::Pilot => 5,
                Subset::Sample => 20,
                Subset::Full => eligible.len(),
            };
            if eligible.len() < limit.max(10) {
                return Err(invalid(
                    "not enough intact top-level sessions for subset and ten controls",
                ));
            }
            let replay = eligible
                .iter()
                .take(limit)
                .map(|t| planned(t))
                .collect::<Result<_, _>>()?;
            let control = eligible
                .iter()
                .take(10)
                .map(|t| planned(t))
                .collect::<Result<_, _>>()?;
            save_json(&args.corpus.out, &Plan { replay, control })
        }
        HistoryCommand::ConfigCheck { a, b } => check_config(a, b),
        HistoryCommand::Report {
            baseline,
            measurements,
            out,
        } => {
            let baseline: Baseline = serde_json::from_slice(&fs::read(baseline)?)?;
            let measurements: Measurements = serde_json::from_slice(&fs::read(measurements)?)?;
            // Build the whole report before publishing; invalid input cannot leave a partial artifact.
            let mut content = Vec::new();
            write_report(&baseline, &measurements, &mut content)?;
            private_write(out, &content)
        }
    }
}
fn planned(transcript: &Transcript) -> Result<PlannedSession, BenchError> {
    let start_commit = if let Some(commit) = &transcript.spec.start_commit {
        validate_commit(commit)?;
        Some(commit.clone())
    } else {
        let log = fs::read_to_string(&transcript.spec.git_log)?;
        anchor(&parse_git_log(&log)?, transcript.started_ms)
    };
    let end_commit = transcript
        .spec
        .end_commit
        .as_ref()
        .ok_or_else(|| invalid("top-level session lacks original end commit"))?;
    validate_commit(end_commit)?;
    let prompts = extract_prompts(&transcript.records);
    if prompts.is_empty() || prompts.iter().any(|p| p.model.is_none()) {
        return Err(invalid(
            "session lacks typed prompts or recorded prompt model",
        ));
    }
    Ok(PlannedSession {
        id: transcript.spec.id.clone(),
        start_commit,
        end_commit: end_commit.clone(),
        prompts,
    })
}
/// Manifest, loadable transcripts, and the reason each skipped session was left out.
type Corpus = (Manifest, Vec<Transcript>, BTreeMap<String, String>);

fn corpus(path: &Path) -> Result<Corpus, BenchError> {
    let mut manifest: Manifest = serde_json::from_slice(&fs::read(path)?)?;
    let root = path
        .parent()
        .ok_or_else(|| invalid("manifest has no parent directory"))?
        .canonicalize()?;
    let mut transcripts = Vec::new();
    let mut skipped = BTreeMap::new();
    let mut ids = std::collections::BTreeSet::new();
    for spec in &mut manifest.sessions {
        if spec.id.is_empty() || !ids.insert(spec.id.clone()) {
            return Err(invalid("empty or duplicate session ID"));
        }
        spec.path = bundle_file(&root, &spec.path)?;
        if spec.parent.is_none() && spec.start_commit.is_none() {
            spec.git_log = bundle_file(&root, &spec.git_log)?;
        }
        match load_transcript(spec) {
            Ok(transcript) => transcripts.push(transcript),
            Err(BenchError::Json(_)) => {
                skipped.insert(spec.id.clone(), "malformed JSON transcript".into());
            }
            Err(BenchError::Invalid(_)) => {
                skipped.insert(
                    spec.id.clone(),
                    "invalid transcript metadata or usage".into(),
                );
            }
            Err(error) => return Err(error),
        }
    }
    // Exclude descendants of malformed parents as well, and retain their content-free reason.
    loop {
        let missing: Vec<_> = transcripts
            .iter()
            .filter(|t| {
                t.spec
                    .parent
                    .as_ref()
                    .is_some_and(|p| skipped.contains_key(p))
            })
            .map(|t| t.spec.id.clone())
            .collect();
        if missing.is_empty() {
            break;
        }
        for id in &missing {
            skipped.insert(id.clone(), "spawning transcript was skipped".into());
        }
        transcripts.retain(|t| !missing.contains(&t.spec.id));
    }
    Ok((manifest, transcripts, skipped))
}
fn bundle_file(root: &Path, path: &Path) -> Result<PathBuf, BenchError> {
    let resolved = root.join(path).canonicalize()?;
    if !resolved.starts_with(root) || !resolved.is_file() {
        return Err(invalid("bundle path is not an in-bundle file"));
    }
    Ok(resolved)
}
fn save_json(path: &Path, value: &impl Serialize) -> Result<(), BenchError> {
    private_write(path, &serde_json::to_vec_pretty(value)?)
}
fn private_write(path: &Path, content: &[u8]) -> Result<(), BenchError> {
    use std::io::Write;
    // A reused output keeps its old mode, so write a fresh 0600 file beside it and rename.
    let name = path
        .file_name()
        .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let temp = path.with_file_name(format!(".{name}.{}.{nanos}.tmp", std::process::id()));
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let written = options
        .open(&temp)
        .and_then(|mut file| file.write_all(content).and_then(|()| file.sync_all()))
        .and_then(|()| fs::rename(&temp, path));
    if let Err(error) = written {
        let _ = fs::remove_file(&temp);
        return Err(error.into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::private_write;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn private_write_replaces_a_world_readable_file_with_a_private_one() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("plan.json");
        fs::write(&path, b"old").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
        private_write(&path, b"raw prompt").unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"raw prompt");
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        let leftovers = fs::read_dir(dir.path()).unwrap().count();
        assert_eq!(leftovers, 1, "temp file must not remain");
    }
}
