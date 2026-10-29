//! Complete command-line grammar from SPEC 10.4, 11.1, and 12.

use std::ffi::OsString;
use std::path::PathBuf;

use clap::{Args, FromArgMatches, Parser, Subcommand, ValueEnum};
use tkm_proto::Host;

/// Manage tokenimum installation, evidence, configuration, and diagnostics.
#[derive(Debug, Parser)]
#[command(name = "tkm", version)]
pub struct Cli {
    /// Command to execute.
    #[command(subcommand)]
    pub command: Command,
}

/// Every supported top-level command.
#[derive(Debug, Subcommand)]
pub enum Command {
    /// Private history replay benchmark.
    #[command(hide = true)]
    Bench(crate::replay_bench::BenchArgs),
    /// Detect harnesses and install tokenimum.
    Init(InitArgs),
    /// Remove tokenimum's recorded entries and shims.
    Uninstall(UninstallArgs),
    /// Show repository evidence, spend, and daemon state.
    Status(StatusArgs),
    /// Open the live terminal monitor.
    Top(TopArgs),
    /// Start or open the local web dashboard.
    Dashboard(DashboardArgs),
    /// Explain an action using its stored evidence.
    Why(WhyArgs),
    /// List facts with evidence, or retire a fact.
    Facts(FactsArgs),
    /// Inspect, apply, or restore tool deny-list entries.
    Prune(PruneArgs),
    /// Show evidence of skills loaded and used.
    Skills(SkillsArgs),
    /// Inspect and edit layered configuration.
    Config(ConfigArgs),
    /// Show decision models, availability, and calibration.
    Models(ModelsArgs),
    /// Fit decision thresholds using labelled decisions.
    Calibrate(CalibrateArgs),
    /// Replay mechanisms over a recorded session or fixture.
    Replay(ReplayArgs),
    /// Wrap a harness for fold-only session tracking.
    Run(RunArgs),
    /// Diagnose the daemon, database, hooks, and harnesses.
    Doctor(DoctorArgs),
    /// Export the current repository's state as JSON.
    Export(ExportArgs),
}

/// Installation options.
#[derive(Debug, Args)]
// These independent switches mirror the installation interface in SPEC 11.1.
#[allow(clippy::struct_excessive_bools)]
pub struct InitArgs {
    /// Accept the installation plan without interactive prompts.
    #[arg(long)]
    pub yes: bool,
    /// Show planned changes without writing files.
    #[arg(long)]
    pub dry_run: bool,
    /// Install only the comma-separated harness names.
    #[arg(long, value_delimiter = ',', value_name = "LIST")]
    pub only: Vec<Host>,
    /// Use this agent home for the selected host.
    #[arg(long, value_name = "DIR")]
    pub home: Option<PathBuf>,
    /// Select the host associated with an explicit agent home.
    #[arg(long, value_name = "HOST", value_parser = |s: &str| if s == "claude-code" { Ok(Host::Claude) } else { s.parse::<Host>() })]
    pub host: Option<Host>,
    /// Include superorchestrator account homes in detection.
    #[arg(long)]
    pub superorchestrator: bool,
    /// Use fold-only mode where a shim would otherwise be installed.
    #[arg(long)]
    pub no_shims: bool,
    /// Select the named configuration preset.
    #[arg(long, value_name = "NAME")]
    pub preset: Option<String>,
}

/// Uninstallation options.
#[derive(Debug, Args)]
pub struct UninstallArgs {
    /// Remove entries only for this host.
    #[arg(long, value_name = "HOST")]
    pub host: Option<Host>,
    /// Show removals without writing files.
    #[arg(long)]
    pub dry_run: bool,
}

/// Status report options.
#[derive(Debug, Args)]
pub struct StatusArgs {
    /// Include every repository.
    #[arg(long)]
    pub all: bool,
    /// Emit the report as JSON.
    #[arg(long)]
    pub json: bool,
}

/// Terminal monitor options.
#[derive(Debug, Default, Args)]
pub struct TopArgs {}

/// Local web dashboard options.
#[derive(Debug, Default, Args)]
pub struct DashboardArgs {
    /// Print the local dashboard URL without opening a browser.
    #[arg(long)]
    pub no_open: bool,
}

/// Action explanation options.
#[derive(Debug, Args)]
pub struct WhyArgs {
    /// Event identifier, or `last` for the most recent action.
    #[arg(value_name = "EVENT", default_value = "last")]
    pub event: String,
    /// Limit the explanation to this session.
    #[arg(long, value_name = "ID")]
    pub session: Option<String>,
}

/// Fact listing and retirement options.
#[derive(Debug, Args)]
pub struct FactsArgs {
    /// Include candidate and retired facts for this repository.
    #[arg(long, conflicts_with = "brief")]
    pub all: bool,
    /// Print a compact fact list suitable for harness context.
    #[arg(long)]
    pub brief: bool,
    /// Optional fact mutation.
    #[command(subcommand)]
    pub command: Option<FactsCommand>,
}

/// Supported fact mutations.
#[derive(Debug, Subcommand)]
pub enum FactsCommand {
    /// Retire the fact with this identifier.
    Forget(ForgetArgs),
}

/// Fact retirement options.
#[derive(Debug, Args)]
pub struct ForgetArgs {
    /// Identifier of the fact to retire.
    pub id: String,
}

/// Deny-list inspection and mutation options.
#[derive(Debug, Args)]
pub struct PruneArgs {
    /// Apply proposals or restore tools; omit to list proposals.
    #[command(subcommand)]
    pub command: Option<PruneCommand>,
}

/// Supported deny-list mutations.
#[derive(Debug, Subcommand)]
pub enum PruneCommand {
    /// Apply the proposed tool deny-list.
    Apply(PruneApplyArgs),
    /// Restore one tool or every tokenimum-owned deny entry.
    Restore(PruneRestoreArgs),
}

/// Deny-list apply options.
#[derive(Debug, Default, Args)]
pub struct PruneApplyArgs {}

/// Deny-list restore options.
#[derive(Debug, Args)]
pub struct PruneRestoreArgs {
    /// Tool to restore; omit to restore all owned entries.
    pub tool: Option<String>,
}

/// Skill evidence report options.
#[derive(Debug, Args)]
pub struct SkillsArgs {
    /// Emit skill evidence as JSON.
    #[arg(long)]
    pub json: bool,
}

/// Configuration command selection.
#[derive(Debug, Args)]
pub struct ConfigArgs {
    /// Configuration operation to perform.
    #[command(subcommand)]
    pub command: ConfigCommand,
}

/// Layered configuration operations from SPEC 10.4.
#[derive(Debug, Subcommand)]
pub enum ConfigCommand {
    /// Show merged configuration and optionally its source layers.
    Show(ConfigShowArgs),
    /// Read the effective value of a configuration key.
    Get(ConfigKeyArgs),
    /// Set a validated value in one configuration layer.
    Set(ConfigSetArgs),
    /// Remove a key from one configuration layer.
    Unset(ConfigUnsetArgs),
    /// Switch the selected preset.
    Preset(ConfigPresetArgs),
    /// Explain a key's meaning, default, allowed values, and source.
    Explain(ConfigKeyArgs),
    /// Open a configuration file in the editor and validate it on save.
    Edit(ConfigEditArgs),
    /// Show differences from the selected preset.
    Diff(ConfigDiffArgs),
}

/// Configuration display options.
#[derive(Debug, Args)]
pub struct ConfigShowArgs {
    /// Include the source layer of every effective value.
    #[arg(long)]
    pub effective: bool,
}

/// A dotted configuration key.
#[derive(Debug, Args)]
pub struct ConfigKeyArgs {
    /// Dotted key, such as `loop.mode`.
    pub key: String,
}

/// Mutually exclusive configuration write layers.
#[derive(Debug, Args)]
#[group(multiple = false)]
pub struct ConfigLayerArgs {
    /// Edit the user-global configuration file.
    #[arg(long)]
    pub global: bool,
    /// Edit the shared repository configuration file.
    #[arg(long)]
    pub repo: bool,
    /// Edit the repository-local configuration file.
    #[arg(long)]
    pub local: bool,
}

/// Configuration assignment options.
#[derive(Debug, Args)]
pub struct ConfigSetArgs {
    /// Dotted configuration key.
    pub key: String,
    /// Value validated against the configuration schema.
    pub value: String,
    /// Layer to edit.
    #[command(flatten)]
    pub layer: ConfigLayerArgs,
}

/// Configuration removal options.
#[derive(Debug, Args)]
pub struct ConfigUnsetArgs {
    /// Dotted configuration key to remove.
    pub key: String,
    /// Layer to edit.
    #[command(flatten)]
    pub layer: ConfigLayerArgs,
}

/// Preset selection options.
#[derive(Debug, Args)]
pub struct ConfigPresetArgs {
    /// Preset name: observe, balanced, aggressive, or offline.
    pub name: String,
    /// Select the preset for this repository.
    #[arg(long)]
    pub repo: bool,
}

/// Configuration editor options.
#[derive(Debug, Args)]
pub struct ConfigEditArgs {
    /// Edit the user-global configuration file.
    #[arg(long, conflicts_with = "repo")]
    pub global: bool,
    /// Edit the shared repository configuration file.
    #[arg(long)]
    pub repo: bool,
}

/// Preset comparison options.
#[derive(Debug, Default, Args)]
pub struct ConfigDiffArgs {}

/// Model listing and availability probe options.
#[derive(Debug, Args)]
pub struct ModelsArgs {
    /// Optional live availability probe.
    #[command(subcommand)]
    pub command: Option<ModelsCommand>,
}

/// Model diagnostic operations.
#[derive(Debug, Subcommand)]
pub enum ModelsCommand {
    /// Probe the configured decision models for availability.
    Probe(ModelsProbeArgs),
}

/// Model availability probe options.
#[derive(Debug, Default, Args)]
pub struct ModelsProbeArgs {}

/// Decision threshold calibration options.
#[derive(Debug, Args)]
pub struct CalibrateArgs {
    /// Calibrate only this decision purpose; omit for all purposes.
    #[arg(long, value_enum)]
    pub purpose: Option<CalibrationPurpose>,
    /// Fit against offline fixture outcomes instead of stored decisions.
    #[arg(long)]
    pub fixtures: Option<std::path::PathBuf>,
}

/// Supported calibration purposes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum CalibrationPurpose {
    /// Loop guard decisions.
    Loop,
    /// Fact promotion and filtering decisions.
    Facts,
    /// Context relevance decisions.
    Relevance,
}

/// Recorded session replay options.
#[derive(Debug, Args)]
pub struct ReplayArgs {
    /// Run the offline decision bench for a fixture directory.
    #[arg(long)]
    pub bench: bool,
    /// Refresh bench responses using the configured live decision models.
    #[arg(long, requires = "bench")]
    pub record: bool,
    /// Session identifier or fixture path.
    #[arg(value_name = "SESSION_OR_FIXTURE")]
    pub session_or_fixture: String,
}

/// Fold-only harness wrapper options.
#[derive(Debug)]
pub struct RunArgs {
    /// Harness executable to launch.
    pub harness: String,
    /// Arguments forwarded verbatim to the harness, including leading hyphens.
    pub args: Vec<OsString>,
}

// Start trailing capture at the harness itself. With separate clap positionals,
// `tkm run claude --help` consumes the wrapper's help flag before args begin.
#[derive(Debug, Args)]
#[command(override_usage = "tkm run <HARNESS> [ARGS]...")]
struct RunArgv {
    /// Harness executable followed by arguments to forward verbatim.
    #[arg(value_name = "HARNESS", num_args = 1.., required = true, trailing_var_arg = true)]
    argv: Vec<OsString>,
}

impl RunArgs {
    fn from_argv(raw: RunArgv) -> Result<Self, clap::Error> {
        let mut argv = raw.argv.into_iter();
        let harness = argv.next().ok_or_else(|| {
            clap::Error::raw(
                clap::error::ErrorKind::MissingRequiredArgument,
                "a harness executable is required",
            )
        })?;
        let harness = harness.into_string().map_err(|_| {
            clap::Error::raw(
                clap::error::ErrorKind::InvalidUtf8,
                "the harness executable name must be UTF-8",
            )
        })?;
        let mut argv = argv.peekable();
        // Consume the wrapper's optional separator; subsequent separators belong to the harness.
        if argv.peek().is_some_and(|arg| arg == "--") {
            let _ = argv.next();
        }
        Ok(Self {
            harness,
            args: argv.collect(),
        })
    }
}

impl FromArgMatches for RunArgs {
    fn from_arg_matches(matches: &clap::ArgMatches) -> Result<Self, clap::Error> {
        Self::from_argv(RunArgv::from_arg_matches(matches)?)
    }

    fn update_from_arg_matches(&mut self, matches: &clap::ArgMatches) -> Result<(), clap::Error> {
        *self = Self::from_arg_matches(matches)?;
        Ok(())
    }
}

impl Args for RunArgs {
    fn group_id() -> Option<clap::Id> {
        RunArgv::group_id()
    }

    fn augment_args(command: clap::Command) -> clap::Command {
        RunArgv::augment_args(command)
    }

    fn augment_args_for_update(command: clap::Command) -> clap::Command {
        RunArgv::augment_args_for_update(command)
    }
}

/// Diagnostic report options.
#[derive(Debug, Args)]
pub struct DoctorArgs {
    /// Probe a host in an isolated HOME; without HOST, probe decision models.
    #[arg(long, num_args = 0..=1, default_missing_value = "models", value_name = "HOST")]
    pub probe: Option<String>,
}

/// Repository state export options.
#[derive(Debug, Args)]
pub struct ExportArgs {
    /// Include stored argument and output digests in the export.
    #[arg(long)]
    pub show_digests: bool,
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    fn run_args(args: &[&str]) -> RunArgs {
        let cli = Cli::try_parse_from(args).unwrap();
        let Command::Run(args) = cli.command else {
            panic!("expected run command");
        };
        args
    }

    #[test]
    fn run_preserves_args_after_separator() {
        let args = run_args(&["tkm", "run", "claude", "--", "--flag", "x"]);
        assert_eq!(args.harness, "claude");
        assert_eq!(args.args, ["--flag", "x"]);
    }

    #[test]
    fn run_preserves_hyphenated_args_without_separator() {
        let args = run_args(&["tkm", "run", "claude", "--model", "y"]);
        assert_eq!(args.args, ["--model", "y"]);
    }

    #[test]
    fn run_preserves_help_and_version_after_harness() {
        for flag in ["--help", "-h", "--version", "-V"] {
            let args = run_args(&["tkm", "run", "claude", flag]);
            assert_eq!(args.args, [flag]);
        }
    }

    #[test]
    fn run_preserves_nested_separator_and_values() {
        let args = run_args(&[
            "tkm",
            "run",
            "claude",
            "prompt,with,commas",
            "--",
            "--help",
            "a b",
        ]);
        assert_eq!(args.args, ["prompt,with,commas", "--", "--help", "a b"]);
    }

    #[cfg(unix)]
    #[test]
    fn run_preserves_non_utf8_args() {
        use std::os::unix::ffi::OsStringExt;

        let value = OsString::from_vec(vec![0xff]);
        let cli = Cli::try_parse_from([
            OsString::from("tkm"),
            OsString::from("run"),
            OsString::from("claude"),
            value.clone(),
        ])
        .unwrap();
        let Command::Run(args) = cli.command else {
            panic!("expected run command");
        };
        assert_eq!(args.args, [value]);
    }

    #[test]
    fn init_accepts_all_install_flags() {
        let cli = Cli::try_parse_from([
            "tkm",
            "init",
            "--yes",
            "--dry-run",
            "--only",
            "claude,codex",
            "--home",
            "/fake/agent",
            "--host",
            "claude",
            "--superorchestrator",
            "--no-shims",
            "--preset",
            "offline",
        ])
        .unwrap();
        let Command::Init(args) = cli.command else {
            panic!("expected init command");
        };
        assert!(args.yes && args.dry_run && args.superorchestrator && args.no_shims);
        assert_eq!(args.only, [Host::Claude, Host::Codex]);
        assert_eq!(args.host, Some(Host::Claude));
        assert_eq!(args.home, Some(PathBuf::from("/fake/agent")));
        assert_eq!(args.preset.as_deref(), Some("offline"));
    }

    #[test]
    fn config_layer_flags_are_mutually_exclusive() {
        for args in [
            vec![
                "tkm",
                "config",
                "set",
                "loop.mode",
                "off",
                "--global",
                "--repo",
            ],
            vec!["tkm", "config", "unset", "loop.mode", "--repo", "--local"],
            vec!["tkm", "config", "edit", "--global", "--repo"],
        ] {
            assert!(Cli::try_parse_from(args).is_err());
        }
    }

    #[test]
    fn facts_modes_are_mutually_exclusive() {
        assert!(Cli::try_parse_from(["tkm", "facts", "--all", "--brief"]).is_err());
    }

    #[test]
    fn remaining_spec_flags_parse() {
        for args in [
            vec!["tkm", "status", "--all", "--json"],
            vec!["tkm", "uninstall", "--host", "pi", "--dry-run"],
            vec!["tkm", "why", "last", "--session", "s-1"],
            vec!["tkm", "facts", "--brief"],
            vec!["tkm", "facts", "--all"],
            vec!["tkm", "config", "show", "--effective"],
            vec!["tkm", "config", "get", "loop.mode"],
            vec!["tkm", "config", "set", "loop.mode", "off", "--global"],
            vec!["tkm", "config", "set", "loop.mode", "off", "--repo"],
            vec!["tkm", "config", "set", "loop.mode", "off", "--local"],
            vec!["tkm", "config", "unset", "loop.mode", "--local"],
            vec!["tkm", "config", "preset", "observe", "--repo"],
            vec!["tkm", "config", "explain", "loop.mode"],
            vec!["tkm", "config", "edit", "--repo"],
            vec!["tkm", "config", "diff"],
            vec!["tkm", "calibrate", "--purpose", "loop"],
            vec!["tkm", "calibrate", "--purpose", "facts"],
            vec!["tkm", "calibrate", "--purpose", "relevance"],
        ] {
            assert!(Cli::try_parse_from(&args).is_ok(), "{args:?}");
        }
    }
}
