#![forbid(unsafe_code)]
#![cfg(target_os = "linux")]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::pedantic)]
mod support_init;
use support_init::{Fixture, stdout};
use tkm_proto::Host;

#[test]
fn fake_host_subset_records_arrivals_and_marks_missing_events_idle_fold() {
    let f = Fixture::new();
    f.init();
    f.script(
        "agent",
        r#"
if [ "$1" = --version ]; then printf 'fixture 1.0\n'; exit 0; fi
[ "$HOME" != "$REAL_HOME" ] || exit 9
[ -f "$HOME/.cursor/hooks.json" ] || exit 8
tkm-hook --host cursor --event session_start --host-event sessionStart
tkm-hook --host cursor --event tool_done --host-event postToolUse
printf '%s' "$HOME" > "$PROBE_HOME_RECORD"
"#,
    );
    let record = f.temp.path().join("probe-home");
    let output = f
        .command(&["doctor", "--probe", "cursor"])
        .env("REAL_HOME", &f.home)
        .env("PROBE_HOME_RECORD", &record)
        .output()
        .unwrap();
    let report = stdout(&output);
    assert!(report.contains("cursor probe:"), "{output:?}");
    let store = tkm_store::Store::open(&f.paths.db).unwrap();
    let rows = store.host_event_probes(Host::Cursor).unwrap();
    assert!(rows.iter().any(|r| r.native == "sessionStart" && r.arrived));
    assert!(
        rows.iter()
            .any(|r| r.native == "beforeSubmitPrompt" && !r.arrived)
    );
    assert!(rows.iter().any(|r| r.native == "stop" && !r.arrived));
    let home = std::fs::read_to_string(record).unwrap();
    assert!(
        !std::path::Path::new(&home).exists(),
        "scratch HOME must be cleaned"
    );
    assert!(
        !store.host_fold_only(Host::Cursor).unwrap(),
        "partial coverage falls back per event"
    );
}

#[test]
fn missing_binary_forces_fold_only_and_records_every_event_missing() {
    let f = Fixture::new();
    f.init();
    std::fs::remove_file(f.bin.join("claude")).unwrap();
    let output = f.run(&["doctor", "--probe", "claude"]);
    assert!(stdout(&output).contains("binary unavailable"), "{output:?}");
    let store = tkm_store::Store::open(&f.paths.db).unwrap();
    assert!(store.host_fold_only(Host::Claude).unwrap());
    assert!(
        store
            .host_event_probes(Host::Claude)
            .unwrap()
            .iter()
            .all(|r| !r.arrived)
    );
}

#[test]
fn codewhale_uses_live_observations_without_invoking_exec() {
    let f = Fixture::new();
    f.init();
    f.script(
        "codewhale",
        "[ \"$1\" = --version ] || exit 9\nprintf 'fixture 1.0\\n'\n",
    );
    let output = f.run(&["doctor", "--probe", "deepseek"]);
    assert!(
        stdout(&output).contains("recorded live events"),
        "{output:?}"
    );
    assert!(
        !tkm_store::Store::open(&f.paths.db)
            .unwrap()
            .host_event_probes(Host::Deepseek)
            .unwrap()
            .is_empty()
    );
}
