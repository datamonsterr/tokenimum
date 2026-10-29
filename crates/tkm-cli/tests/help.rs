#![forbid(unsafe_code)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::pedantic)]

mod common;

macro_rules! help_tests {
    ($($name:ident: [$($part:literal),*];)*) => {$(
        #[test]
        fn $name() {
            let path = [$($part),*];
            let mut args = path.to_vec();
            args.push("--help");
            let output = common::tkm(&args);
            assert!(output.status.success(), "{path:?}: {output:?}");
            let stdout = String::from_utf8(output.stdout).unwrap();
            assert!(stdout.contains(*path.last().unwrap_or(&"tkm")), "{stdout}");
        }
    )*};
}

help_tests! {
    root_help: [];
    init_help: ["init"];
    uninstall_help: ["uninstall"];
    status_help: ["status"];
    top_help: ["top"];
    dashboard_help: ["dashboard"];
    why_help: ["why"];
    facts_help: ["facts"];
    facts_forget_help: ["facts", "forget"];
    prune_help: ["prune"];
    prune_apply_help: ["prune", "apply"];
    prune_restore_help: ["prune", "restore"];
    skills_help: ["skills"];
    config_help: ["config"];
    config_show_help: ["config", "show"];
    config_get_help: ["config", "get"];
    config_set_help: ["config", "set"];
    config_unset_help: ["config", "unset"];
    config_preset_help: ["config", "preset"];
    config_explain_help: ["config", "explain"];
    config_edit_help: ["config", "edit"];
    config_diff_help: ["config", "diff"];
    models_help: ["models"];
    models_probe_help: ["models", "probe"];
    calibrate_help: ["calibrate"];
    replay_help: ["replay"];
    run_help: ["run"];
    doctor_help: ["doctor"];
    export_help: ["export"];
}

#[test]
fn version() {
    let output = common::tkm(&["--version"]);
    assert!(output.status.success(), "{output:?}");
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        concat!("tkm ", env!("CARGO_PKG_VERSION"), "\n")
    );
}
