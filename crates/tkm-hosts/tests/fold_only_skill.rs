#![forbid(unsafe_code)]
#![allow(clippy::unwrap_used, clippy::expect_used)]
//! Tests for `fold_only::skill` (SPEC 6.12, ADR 0026).

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use tkm_hosts::AdapterError;
use tkm_hosts::fold_only::skill::{install, render, skill_path, uninstall};
use tkm_proto::Host;

struct Scratch(PathBuf);
impl Scratch {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "tkm-fold-skill-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn render_starts_with_frontmatter_naming_the_skill() {
    let text = render(Host::Claude);
    assert!(
        text.starts_with("---\nname: tokenimum\ndescription: "),
        "{text}"
    );
    let close = text[4..].find("\n---\n").expect("frontmatter closes");
    assert!(close > 0);
}

#[test]
fn render_lists_only_the_two_read_only_commands() {
    let text = render(Host::Pi);
    assert!(text.contains("tkm facts --brief"));
    assert!(text.contains("tkm why <event>"));
    let commands: Vec<_> = text.lines().filter(|l| l.starts_with("tkm ")).collect();
    assert_eq!(commands, ["tkm facts --brief", "tkm why <event>"]);
}

#[test]
fn render_makes_no_claims_about_effects() {
    let lower = render(Host::Opencode).to_lowercase();
    for word in ["save", "saving", "faster", "cheaper", "reduce", "improve"] {
        assert!(!lower.contains(word), "unexpected claim word {word}");
    }
}

#[test]
fn render_has_versioned_marker_after_frontmatter() {
    let text = render(Host::Claude);
    let marker = format!(
        "<!-- tokenimum-skill version={} ",
        env!("CARGO_PKG_VERSION")
    );
    assert!(text.contains(&marker), "{text}");
    assert_eq!(render(Host::Claude), render(Host::Claude));
}

#[test]
fn skill_path_user_scope_per_harness() {
    let home = Path::new("/home/user/.agent");
    let expected = home.join("skills/tokenimum/SKILL.md");
    assert_eq!(skill_path(Host::Claude, home, None), Some(expected.clone()));
    assert_eq!(
        skill_path(Host::Opencode, home, None),
        Some(expected.clone())
    );
    assert_eq!(skill_path(Host::Kimi, home, None), Some(expected));
    for host in [
        Host::Pi,
        Host::Codex,
        Host::Agy,
        Host::Cursor,
        Host::Deepseek,
    ] {
        assert_eq!(skill_path(host, home, None), None, "{host:?}");
    }
}

#[test]
fn skill_path_project_scope_per_harness() {
    let home = Path::new("/home/user/.agent");
    let repo = Path::new("/home/user/project");
    let rel = |dir: &str| repo.join(dir).join("tokenimum/SKILL.md");
    assert_eq!(
        skill_path(Host::Claude, home, Some(repo)),
        Some(rel(".claude/skills"))
    );
    assert_eq!(
        skill_path(Host::Opencode, home, Some(repo)),
        Some(rel(".opencode/skills"))
    );
    assert_eq!(
        skill_path(Host::Pi, home, Some(repo)),
        Some(rel(".agents/skills"))
    );
    assert_eq!(skill_path(Host::Kimi, home, Some(repo)), None);
    assert_eq!(skill_path(Host::Codex, home, Some(repo)), None);
}

#[test]
fn install_writes_file_and_is_idempotent() {
    let scratch = Scratch::new();
    let report = install(Host::Claude, &scratch.0, None, false).unwrap();
    let path = scratch.0.join("skills/tokenimum/SKILL.md");
    assert_eq!(report.changed_files, vec![path.clone()]);
    assert_eq!(fs::read_to_string(&path).unwrap(), render(Host::Claude));
    let again = install(Host::Claude, &scratch.0, None, false).unwrap();
    assert!(again.changed_files.is_empty(), "unexpected report");
}

#[test]
fn install_dry_run_writes_nothing() {
    let scratch = Scratch::new();
    let report = install(Host::Opencode, &scratch.0, None, true).unwrap();
    assert!(report.dry_run);
    assert_eq!(report.changed_files.len(), 1);
    assert!(!scratch.0.join("skills").exists());
}

#[test]
fn install_project_scope_uses_repo_root() {
    let scratch = Scratch::new();
    let home = scratch.0.join("home");
    let repo = scratch.0.join("repo");
    fs::create_dir_all(&repo).unwrap();
    install(Host::Pi, &home, Some(&repo), false).unwrap();
    assert!(repo.join(".agents/skills/tokenimum/SKILL.md").is_file());
    assert!(!home.exists());
}

#[test]
fn install_unverified_harness_is_report_only() {
    let scratch = Scratch::new();
    let report = install(Host::Cursor, &scratch.0, None, false).unwrap();
    assert!(report.changed_files.is_empty(), "unexpected report");
    assert!(!report.instructions.is_empty(), "unexpected report");
    assert_eq!(fs::read_dir(&scratch.0).unwrap().count(), 0);
}

#[test]
fn install_refuses_to_overwrite_a_foreign_skill() {
    let scratch = Scratch::new();
    let path = scratch.0.join("skills/tokenimum/SKILL.md");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, "---\nname: tokenimum\n---\nmine\n").unwrap();
    let err = install(Host::Claude, &scratch.0, None, false).unwrap_err();
    assert!(matches!(err, AdapterError::UnsafeTarget { .. }), "{err:?}");
    assert_eq!(
        fs::read_to_string(&path).unwrap(),
        "---\nname: tokenimum\n---\nmine\n"
    );
}

#[cfg(unix)]
#[test]
fn install_refuses_symlink_out_of_agent_home() {
    let scratch = Scratch::new();
    let outside = Scratch::new();
    let home = scratch.0.join("home");
    fs::create_dir_all(home.join("skills")).unwrap();
    std::os::unix::fs::symlink(&outside.0, home.join("skills/tokenimum")).unwrap();
    let err = install(Host::Claude, &home, None, false).unwrap_err();
    assert!(matches!(err, AdapterError::UnsafeTarget { .. }), "{err:?}");
    assert_eq!(fs::read_dir(&outside.0).unwrap().count(), 0);
}

#[test]
fn uninstall_removes_owned_file_and_empty_directory() {
    let scratch = Scratch::new();
    install(Host::Claude, &scratch.0, None, false).unwrap();
    let report = uninstall(Host::Claude, &scratch.0, None, false).unwrap();
    assert_eq!(
        report.changed_files,
        vec![scratch.0.join("skills/tokenimum/SKILL.md")]
    );
    assert!(!scratch.0.join("skills/tokenimum").exists());
}

#[test]
fn uninstall_dry_run_keeps_file() {
    let scratch = Scratch::new();
    install(Host::Claude, &scratch.0, None, false).unwrap();
    let report = uninstall(Host::Claude, &scratch.0, None, true).unwrap();
    assert!(report.dry_run);
    assert_eq!(report.changed_files.len(), 1);
    assert!(scratch.0.join("skills/tokenimum/SKILL.md").is_file());
}

#[test]
fn uninstall_leaves_file_without_marker() {
    let scratch = Scratch::new();
    let path = scratch.0.join("skills/tokenimum/SKILL.md");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, "---\nname: tokenimum\n---\nhand written\n").unwrap();
    let report = uninstall(Host::Claude, &scratch.0, None, false).unwrap();
    assert!(report.changed_files.is_empty(), "unexpected report");
    assert!(path.is_file());
}

#[test]
fn uninstall_absent_and_unsupported_are_noops() {
    let scratch = Scratch::new();
    for host in [Host::Claude, Host::Codex] {
        let report = uninstall(host, &scratch.0, None, false).unwrap();
        assert_eq!(report.changed_files, Vec::<PathBuf>::new());
    }
}
