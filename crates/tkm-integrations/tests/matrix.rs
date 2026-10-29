#![forbid(unsafe_code)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::pedantic)]

use std::{
    collections::BTreeMap,
    fs,
    os::unix::fs::{PermissionsExt, symlink},
    sync::Arc,
};
use tkm_core::model::RepoKey;
use tkm_integrations::{
    SkillSource,
    detect::{Mode, SystemClock},
    otherskill::read_key,
    runtime::Integrations,
};
use tkm_proto::Host;

fn script(dir: &std::path::Path, name: &str, body: &str) {
    let path = dir.join(name);
    fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

#[test]
fn matrix_detects_and_falls_back_after_removal() {
    for (skills, orchestrator) in [(false, false), (true, false), (false, true), (true, true)] {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("home");
        fs::create_dir_all(home.join(".config/otherskill")).unwrap();
        fs::create_dir_all(home.join(".config/superorchestrator")).unwrap();
        fs::write(home.join(".config/otherskill/otherskill.config.json"), "{}").unwrap();
        if skills {
            script(
                dir.path(),
                "otherskill",
                "if [ \"$1\" = --help ]; then exit 0; fi; printf '%s' '{\"status\":\"ok\",\"selected\":[{\"name\":\"a\",\"description\":\"first\"},{\"name\":\"a\",\"description\":\"second\"}]}'",
            );
        }
        if orchestrator {
            script(
                dir.path(),
                "superorchestrator",
                "if [ \"$1\" = --version ]; then exit 0; fi; printf '[]'",
            );
        }
        let runtime = Integrations::new(
            Mode::Auto,
            Mode::Auto,
            dir.path().as_os_str().to_owned(),
            home,
            BTreeMap::new(),
            Arc::new(SystemClock),
        );
        runtime.detect_now(dir.path());
        assert_eq!(runtime.otherskill_enabled(), skills);
        assert_eq!(runtime.superorchestrator_enabled(), orchestrator);
        let config = tkm_core::prune::PruneConfig {
            superorchestrator_adapter_enabled: runtime.superorchestrator_enabled(),
            ..Default::default()
        };
        assert_eq!(
            tkm_core::prune::is_protected(
                "mcp__superorchestrator__report_progress",
                Host::Claude,
                &config
            ),
            orchestrator
        );
        let mut evidence = tkm_core::prune::PruneEvidence::default();
        evidence.tools.insert(
            "mcp__superorchestrator__report_progress".into(),
            tkm_core::prune::ToolEvidence {
                prompts: 20,
                sessions: 5,
                ..Default::default()
            },
        );
        let result = tkm_core::prune::decide(
            &evidence,
            Host::Claude,
            &config,
            None,
            tkm_core::model::Timestamp(0),
        );
        if orchestrator {
            assert!(result.candidates.is_empty());
        }
        let candidates = runtime.candidates(&RepoKey::default(), Host::Claude, "prompt");
        assert_eq!(candidates.len(), usize::from(skills));
        let env = BTreeMap::from([("SUPERORCHESTRATOR_RUN_ID".to_owned(), "run".to_owned())]);
        assert_eq!(runtime.delegated_run(dir.path(), &env).is_some(), true);
        if skills {
            fs::remove_file(dir.path().join("otherskill")).unwrap();
        }
        if orchestrator {
            fs::remove_file(dir.path().join("superorchestrator")).unwrap();
        }
        runtime.detect_now(dir.path());
        assert!(!runtime.otherskill_enabled());
        assert!(!runtime.superorchestrator_enabled());
        assert!(
            runtime
                .candidates(&RepoKey::default(), Host::Claude, "prompt")
                .is_empty()
        );
        assert_eq!(
            runtime.delegated_run(dir.path(), &env).as_deref(),
            Some("run")
        );
    }
}

#[test]
fn credential_requires_private_regular_file() {
    let dir = tempfile::tempdir().unwrap();
    let parent = dir.path().join(".config/otherskill");
    fs::create_dir_all(&parent).unwrap();
    let path = parent.join("credentials.json");
    fs::write(&path, r#"{"openrouter":"key"}"#).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
    assert_eq!(read_key(dir.path()).as_deref(), Some("key"));
    fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
    assert_eq!(read_key(dir.path()), None);
    fs::remove_file(&path).unwrap();
    let target = parent.join("target");
    fs::write(&target, r#"{"openrouter":"key"}"#).unwrap();
    fs::set_permissions(&target, fs::Permissions::from_mode(0o600)).unwrap();
    symlink(&target, &path).unwrap();
    assert_eq!(read_key(dir.path()), None);
}

#[test]
fn failed_skill_process_enters_backoff_and_fails_open() {
    for body in ["exit 1", "printf garbage", "/bin/sleep 3"] {
        let dir = tempfile::tempdir().unwrap();
        script(
            dir.path(),
            "otherskill",
            &format!("if [ \"$1\" = --help ]; then exit 0; fi; {body}"),
        );
        let runtime = Integrations::new(
            Mode::Off,
            Mode::On,
            dir.path().as_os_str().to_owned(),
            dir.path().to_path_buf(),
            BTreeMap::new(),
            Arc::new(SystemClock),
        );
        runtime.detect_now(dir.path());
        assert!(
            runtime
                .candidates(&RepoKey::default(), Host::Claude, "prompt")
                .is_empty()
        );
        assert!(!runtime.otherskill_enabled());
        assert!(matches!(
            runtime.status()[1].state,
            tkm_integrations::detect::AdapterState::Backoff { .. }
        ));
    }
}

#[test]
fn runtime_delegated_escalation_reaches_mcp() {
    use tkm_integrations::{EscalationEvent, Escalator};
    let dir = tempfile::tempdir().unwrap();
    let log = dir.path().join("rpc");
    script(
        dir.path(),
        "superorchestrator",
        &format!(
            "if [ \"$1\" = --version ]; then exit 0; fi; if [ \"$1\" = runs ]; then printf '[]'; exit 0; fi; while IFS= read -r line; do printf '%s\\n' \"$line\" >> '{}'; case \"$line\" in *'\"id\":1'*) echo '{{\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{{\"protocolVersion\":\"2025-06-18\"}}}}';; *'\"id\":2'*) echo '{{\"jsonrpc\":\"2.0\",\"id\":2,\"result\":{{}}}}';; esac; done",
            log.display()
        ),
    );
    let runtime = Integrations::new(
        Mode::On,
        Mode::Off,
        dir.path().as_os_str().to_owned(),
        dir.path().to_path_buf(),
        BTreeMap::new(),
        Arc::new(SystemClock),
    );
    runtime.detect_now(dir.path());
    let env = BTreeMap::from([
        ("SUPERORCHESTRATOR_RUN_ID".to_owned(), "run".to_owned()),
        (
            "SUPERORCHESTRATOR_RUN_TOKEN".to_owned(),
            "secret".to_owned(),
        ),
    ]);
    runtime.observe_session(Host::Claude, "session", dir.path(), &env);
    assert!(runtime.handles(Host::Claude, "session"));
    runtime.escalate(&EscalationEvent {
        host: Host::Claude,
        session_id: "session".into(),
        ..EscalationEvent::default()
    });
    let calls = fs::read_to_string(log).unwrap();
    assert!(calls.contains("report_progress"));
    assert!(calls.contains("tokenimum.loop"));
    assert!(!calls.contains("secret"));
}

#[test]
fn session_observed_before_detection_is_registered_after_enable() {
    use tkm_integrations::Escalator;
    let dir = tempfile::tempdir().unwrap();
    script(dir.path(), "superorchestrator", "printf '[]'");
    let runtime = Integrations::new(
        Mode::On,
        Mode::Off,
        dir.path().as_os_str().to_owned(),
        dir.path().to_path_buf(),
        BTreeMap::new(),
        Arc::new(SystemClock),
    );
    let env = BTreeMap::from([
        ("SUPERORCHESTRATOR_RUN_ID".into(), "run".into()),
        ("SUPERORCHESTRATOR_RUN_TOKEN".into(), "secret".into()),
    ]);
    runtime.observe_session(Host::Claude, "s", dir.path(), &env);
    assert!(!runtime.handles(Host::Claude, "s"));
    runtime.detect_now(dir.path());
    assert!(runtime.handles(Host::Claude, "s"));
}

#[test]
fn forgetting_session_removes_registered_run() {
    use tkm_integrations::Escalator;
    let dir = tempfile::tempdir().unwrap();
    script(dir.path(), "superorchestrator", "printf '[]'");
    let runtime = Integrations::new(
        Mode::On,
        Mode::Off,
        dir.path().as_os_str().to_owned(),
        dir.path().to_path_buf(),
        BTreeMap::new(),
        Arc::new(SystemClock),
    );
    runtime.detect_now(dir.path());
    let env = BTreeMap::from([
        ("SUPERORCHESTRATOR_RUN_ID".into(), "run".into()),
        ("SUPERORCHESTRATOR_RUN_TOKEN".into(), "secret".into()),
    ]);
    runtime.observe_session(Host::Claude, "s", dir.path(), &env);
    assert!(runtime.handles(Host::Claude, "s"));
    runtime.forget_session(Host::Claude, "s");
    assert!(!runtime.handles(Host::Claude, "s"));
}
