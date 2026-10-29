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
            "tkm", "run", "claude", "prompt,with,commas", "--", "--help", "a b",
        ]);
        assert_eq!(
            args.args,
            ["prompt,with,commas", "--", "--help", "a b"]
        );
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
            "tkm", "init", "--yes", "--dry-run", "--only", "claude,codex", "--home",
            "/fake/agent", "--host", "claude", "--superorchestrator", "--no-shims",
            "--preset", "offline",
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
            vec!["tkm", "config", "set", "loop.mode", "off", "--global", "--repo"],
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
