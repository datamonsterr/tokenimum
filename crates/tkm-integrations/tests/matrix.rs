#![forbid(unsafe_code)]

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
        let candidates = runtime.candidates(&RepoKey::default(), Host::Claude, "prompt");
        assert_eq!(candidates.len(), usize::from(skills));
        let env = BTreeMap::from([("SUPERORCHESTRATOR_RUN_ID".to_owned(), "run".to_owned())]);
        assert_eq!(
            runtime.delegated_run(dir.path(), &env).is_some(),
            orchestrator
        );
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
        assert!(runtime.delegated_run(dir.path(), &env).is_none());
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
