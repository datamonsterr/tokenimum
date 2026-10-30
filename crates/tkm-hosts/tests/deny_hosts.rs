#![forbid(unsafe_code)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::pedantic)]
use std::{collections::BTreeSet, fs, process::Command};
use tkm_core::model::Host;
use tkm_hosts::writer::deny::update_host_deny;

fn set(names: &[&str]) -> BTreeSet<String> {
    names.iter().map(|name| (*name).to_owned()).collect()
}

fn surfaces() -> [(Host, &'static str, &'static str, &'static str); 4] {
    [
        (
            Host::Claude,
            ".claude/settings.local.json",
            "mcp__example__unused",
            r#"{"permissions":{"deny":["Foreign"]},"untouched": [ 1, 2 ]}"#,
        ),
        (
            Host::Cursor,
            ".cursor/cli.json",
            "Mcp(example:unused)",
            r#"{"permissions":{"deny":["Foreign"]},"untouched": [ 1, 2 ]}"#,
        ),
        (
            Host::Opencode,
            "opencode.json",
            "example_unused",
            r#"{"permission":{"foreign":"deny","existing":"allow"},"untouched": [ 1, 2 ]}"#,
        ),
        (
            Host::Pi,
            ".pi/settings.json",
            "-mcp__example__unused",
            r#"{"defaultTools":["+foreign"],"untouched": [ 1, 2 ]}"#,
        ),
    ]
}

#[test]
fn four_host_deny_roundtrip_preserves_foreign_bytes_and_backup() {
    for (host, relative, token, original) in surfaces() {
        let repo = tempfile::tempdir().unwrap();
        assert!(
            Command::new("git")
                .args(["init", "--quiet"])
                .arg(repo.path())
                .status()
                .unwrap()
                .success()
        );
        let path = repo.path().join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, original).unwrap();
        let desired = set(&["mcp__example__unused"]);
        let report =
            update_host_deny(repo.path(), host, &BTreeSet::new(), &desired, false).unwrap();
        assert_eq!(report.owned, desired);
        let edited = fs::read_to_string(&path).unwrap();
        assert!(edited.contains(token), "{host:?}: {edited}");
        assert!(edited.contains(r#""untouched": [ 1, 2 ]"#));
        let backup = path.with_file_name(format!(
            "{}.tkm-bak",
            path.file_name().unwrap().to_str().unwrap()
        ));
        assert_eq!(fs::read_to_string(&backup).unwrap(), original);
        assert!(
            update_host_deny(repo.path(), host, &report.owned, &desired, false)
                .unwrap()
                .changed_files
                .is_empty()
        );
        update_host_deny(repo.path(), host, &report.owned, &BTreeSet::new(), false).unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), original);
        assert_eq!(fs::read_to_string(backup).unwrap(), original);
    }
}

#[test]
fn four_host_deny_dry_run_does_not_write_and_new_files_are_excluded() {
    for (host, relative, _, _) in surfaces() {
        let repo = tempfile::tempdir().unwrap();
        assert!(
            Command::new("git")
                .args(["init", "--quiet"])
                .arg(repo.path())
                .status()
                .unwrap()
                .success()
        );
        let desired = set(&["mcp__example__unused"]);
        let preview =
            update_host_deny(repo.path(), host, &BTreeSet::new(), &desired, true).unwrap();
        assert_eq!(preview.owned, desired);
        assert!(!repo.path().join(relative).exists());
        update_host_deny(repo.path(), host, &BTreeSet::new(), &desired, false).unwrap();
        let exclude = fs::read_to_string(repo.path().join(".git/info/exclude")).unwrap();
        assert!(exclude.lines().any(|line| line == format!("/{relative}")));
        assert!(
            exclude
                .lines()
                .any(|line| line == format!("/{relative}.tkm-bak"))
        );
    }
}

#[test]
fn opencode_never_overwrites_foreign_permission_values() {
    let repo = tempfile::tempdir().unwrap();
    assert!(
        Command::new("git")
            .args(["init", "--quiet"])
            .arg(repo.path())
            .status()
            .unwrap()
            .success()
    );
    let original = r#"{"permission":{"existing":"allow","granular":{"*":"ask"},"foreign":"deny"}}"#;
    let path = repo.path().join("opencode.json");
    fs::write(&path, original).unwrap();
    let report = update_host_deny(
        repo.path(),
        Host::Opencode,
        &BTreeSet::new(),
        &set(&["existing", "granular", "foreign"]),
        false,
    )
    .unwrap();
    assert!(report.owned.is_empty());
    assert_eq!(fs::read_to_string(path).unwrap(), original);
}

#[test]
fn four_host_deny_refuses_tracked_and_escaping_targets() {
    for (host, relative, _, original) in surfaces() {
        let repo = tempfile::tempdir().unwrap();
        assert!(
            Command::new("git")
                .args(["init", "--quiet"])
                .arg(repo.path())
                .status()
                .unwrap()
                .success()
        );
        let path = repo.path().join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, original).unwrap();
        assert!(
            Command::new("git")
                .current_dir(repo.path())
                .args(["add", "--", relative])
                .status()
                .unwrap()
                .success()
        );
        assert!(
            update_host_deny(
                repo.path(),
                host,
                &BTreeSet::new(),
                &set(&["unused"]),
                false
            )
            .is_err()
        );
        assert_eq!(fs::read_to_string(&path).unwrap(), original);
        #[cfg(unix)]
        {
            assert!(
                Command::new("git")
                    .current_dir(repo.path())
                    .args(["rm", "--cached", "--", relative])
                    .status()
                    .unwrap()
                    .success()
            );
            fs::remove_file(&path).unwrap();
            let outside = tempfile::tempdir().unwrap();
            let target = outside.path().join("settings.json");
            fs::write(&target, original).unwrap();
            std::os::unix::fs::symlink(&target, &path).unwrap();
            assert!(
                update_host_deny(
                    repo.path(),
                    host,
                    &BTreeSet::new(),
                    &set(&["unused"]),
                    false
                )
                .is_err()
            );
            assert_eq!(fs::read_to_string(target).unwrap(), original);
        }
    }
}

#[test]
fn restore_removes_created_containers_and_preserves_later_foreign_changes() {
    for (host, relative, _, _) in surfaces() {
        let repo = tempfile::tempdir().unwrap();
        assert!(
            Command::new("git")
                .args(["init", "--quiet"])
                .arg(repo.path())
                .status()
                .unwrap()
                .success()
        );
        let path = repo.path().join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, r#"{"untouched": [ 1, 2 ]}"#).unwrap();
        let applied = update_host_deny(
            repo.path(),
            host,
            &BTreeSet::new(),
            &set(&["mcp__example__unused"]),
            false,
        )
        .unwrap();
        let edited = fs::read_to_string(&path)
            .unwrap()
            .replace("[ 1, 2 ]", "[ 3, 4 ]");
        fs::write(&path, edited).unwrap();
        update_host_deny(repo.path(), host, &applied.owned, &BTreeSet::new(), false).unwrap();
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            r#"{"untouched": [ 3, 4 ]}"#
        );
    }
}

#[test]
fn restoring_a_new_settings_file_returns_to_absence() {
    for (host, relative, _, _) in surfaces() {
        let repo = tempfile::tempdir().unwrap();
        assert!(
            Command::new("git")
                .args(["init", "--quiet"])
                .arg(repo.path())
                .status()
                .unwrap()
                .success()
        );
        let applied = update_host_deny(
            repo.path(),
            host,
            &BTreeSet::new(),
            &set(&["mcp__example__unused"]),
            false,
        )
        .unwrap();
        update_host_deny(repo.path(), host, &applied.owned, &BTreeSet::new(), false).unwrap();
        assert!(!repo.path().join(relative).exists());
        assert!(
            update_host_deny(repo.path(), host, &BTreeSet::new(), &BTreeSet::new(), false)
                .unwrap()
                .changed_files
                .is_empty()
        );
    }
}

fn pi_repo(original: &str) -> (tempfile::TempDir, std::path::PathBuf) {
    let repo = tempfile::tempdir().unwrap();
    assert!(
        Command::new("git")
            .args(["init", "--quiet"])
            .arg(repo.path())
            .status()
            .unwrap()
            .success()
    );
    let path = repo.path().join(".pi/settings.json");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, original).unwrap();
    (repo, path)
}

#[test]
fn pi_empty_default_tools_stay_disabled_and_restore_exactly() {
    let original = r#"{"defaultTools": [], "untouched": 1}"#;
    let (repo, path) = pi_repo(original);
    let desired = set(&["ls", "bash"]);
    let report =
        update_host_deny(repo.path(), Host::Pi, &BTreeSet::new(), &desired, false).unwrap();
    // `[]` already disables every built-in tool; "-ls" would turn it into an inheriting list.
    assert_eq!(fs::read_to_string(&path).unwrap(), original);
    assert!(report.owned.is_empty());
    update_host_deny(
        repo.path(),
        Host::Pi,
        &report.owned,
        &BTreeSet::new(),
        false,
    )
    .unwrap();
    assert_eq!(fs::read_to_string(&path).unwrap(), original);
}

#[test]
fn pi_explicit_selection_skips_disabled_tools_and_excludes_enabled_ones() {
    let original = r#"{"defaultTools": ["read", "bash"]}"#;
    let (repo, path) = pi_repo(original);
    let desired = set(&["bash", "ls"]);
    let report =
        update_host_deny(repo.path(), Host::Pi, &BTreeSet::new(), &desired, false).unwrap();
    let edited: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    // `ls` is not selected, so it needs no entry. `bash` is selected and is excluded after it.
    assert_eq!(
        edited["defaultTools"],
        serde_json::json!(["read", "bash", "-bash"])
    );
    assert_eq!(report.owned, set(&["bash"]));
    update_host_deny(
        repo.path(),
        Host::Pi,
        &report.owned,
        &BTreeSet::new(),
        false,
    )
    .unwrap();
    assert_eq!(fs::read_to_string(&path).unwrap(), original);
}

#[test]
fn pi_absent_default_tools_use_exclusions_and_restore_exactly() {
    let original = r#"{"untouched": 1}"#;
    let (repo, path) = pi_repo(original);
    let desired = set(&["ls"]);
    let report =
        update_host_deny(repo.path(), Host::Pi, &BTreeSet::new(), &desired, false).unwrap();
    assert!(fs::read_to_string(&path).unwrap().contains(r#""-ls""#));
    update_host_deny(
        repo.path(),
        Host::Pi,
        &report.owned,
        &BTreeSet::new(),
        false,
    )
    .unwrap();
    assert_eq!(fs::read_to_string(&path).unwrap(), original);
}
