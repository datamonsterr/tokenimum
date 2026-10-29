#![forbid(unsafe_code)]
#![allow(clippy::unwrap_used, clippy::expect_used)]
//! Tests for `fold_only::skill` (SPEC 6.12, ADR 0026).

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use tkm_hosts::AdapterError;
use tkm_hosts::fold_only::skill::{SkillOptions, install, render, skill_path, uninstall};
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

fn git(dir: &Path, args: &[&str]) {
    let status = std::process::Command::new("git")
        .args(args)
        .current_dir(dir)
        .status()
        .unwrap();
    assert!(status.success(), "git {args:?}");
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
fn skill_lists_all_read_only_commands() {
    let text = render(Host::Pi);
    for command in [
        "tkm status",
        "tkm status --json",
        "tkm why <event|last>",
        "tkm facts --brief",
        "tkm skills --json",
        "tkm doctor",
    ] {
        assert!(text.contains(command), "missing {command}: {text}");
    }
    assert!(text.contains("only read"));
    assert!(text.contains("change nothing"));
}

#[test]
fn skill_description_claims_no_effect() {
    let text = render(Host::Claude);
    let description = text
        .lines()
        .find(|line| line.starts_with("description:"))
        .unwrap()
        .to_lowercase();
    for word in ["improve", "save", "faster", "fix", "reduce", "cheaper"] {
        assert!(!description.contains(word), "unexpected claim {word}");
    }
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
fn every_harness_has_a_documented_skill_path() {
    let home = Path::new("/home/user/.agent");
    let repo = Path::new("/home/user/project");
    for (host, project) in [
        (Host::Claude, ".claude/skills"),
        (Host::Codex, ".agents/skills"),
        (Host::Agy, ".agents/skills"),
        (Host::Cursor, ".cursor/skills"),
        (Host::Kimi, ".kimi/skills"),
        (Host::Deepseek, ".codewhale/skills"),
        (Host::Opencode, ".opencode/skills"),
        (Host::Pi, ".agents/skills"),
    ] {
        assert_eq!(
            skill_path(host, home, Some(repo)),
            Some(repo.join(project).join("tokenimum/SKILL.md")),
            "{host:?}"
        );
    }
    for host in [
        Host::Claude,
        Host::Codex,
        Host::Cursor,
        Host::Kimi,
        Host::Deepseek,
        Host::Opencode,
        Host::Pi,
    ] {
        assert_eq!(
            skill_path(host, home, None),
            Some(home.join("skills/tokenimum/SKILL.md")),
            "{host:?}"
        );
    }
    assert_eq!(
        skill_path(Host::Agy, home, None),
        Some(home.join("antigravity-cli/skills/tokenimum/SKILL.md"))
    );
}

#[test]
fn installs_read_only_skill_for_each_harness() {
    for host in [
        Host::Claude,
        Host::Codex,
        Host::Agy,
        Host::Cursor,
        Host::Kimi,
        Host::Deepseek,
        Host::Opencode,
        Host::Pi,
    ] {
        let scratch = Scratch::new();
        git(&scratch.0, &["init", "-q"]);
        let report = install(host, &scratch.0, Some(&scratch.0), SkillOptions::default()).unwrap();
        assert_eq!(report.changed_files.len(), 1, "{host:?}");
        let text = fs::read_to_string(&report.changed_files[0]).unwrap();
        assert_eq!(text, render(host));
        assert!(text.contains("tkm facts --brief"));
        assert!(text.contains("tkm why"));
        assert!(!text.to_lowercase().contains("improve"));
    }
}

#[test]
fn install_writes_file_and_is_idempotent() {
    let scratch = Scratch::new();
    let report = install(Host::Claude, &scratch.0, None, SkillOptions::default()).unwrap();
    let path = scratch.0.join("skills/tokenimum/SKILL.md");
    assert_eq!(report.changed_files, vec![path.clone()]);
    assert_eq!(fs::read_to_string(&path).unwrap(), render(Host::Claude));
    let again = install(Host::Claude, &scratch.0, None, SkillOptions::default()).unwrap();
    assert!(again.changed_files.is_empty(), "unexpected report");
}

#[test]
fn install_dry_run_writes_nothing() {
    let scratch = Scratch::new();
    let report = install(
        Host::Opencode,
        &scratch.0,
        None,
        SkillOptions {
            dry_run: true,
            ..SkillOptions::default()
        },
    )
    .unwrap();
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
    git(&repo, &["init", "-q"]);
    install(Host::Pi, &home, Some(&repo), SkillOptions::default()).unwrap();
    assert!(repo.join(".agents/skills/tokenimum/SKILL.md").is_file());
    assert!(!home.exists());
}

#[test]
fn install_refuses_to_overwrite_a_foreign_skill() {
    let scratch = Scratch::new();
    let path = scratch.0.join("skills/tokenimum/SKILL.md");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, "---\nname: tokenimum\n---\nmine\n").unwrap();
    let err = install(Host::Claude, &scratch.0, None, SkillOptions::default()).unwrap_err();
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
    let err = install(Host::Claude, &home, None, SkillOptions::default()).unwrap_err();
    assert!(matches!(err, AdapterError::UnsafeTarget { .. }), "{err:?}");
    assert_eq!(fs::read_dir(&outside.0).unwrap().count(), 0);
}

#[test]
fn uninstall_removes_owned_file_and_empty_directory() {
    let scratch = Scratch::new();
    install(Host::Claude, &scratch.0, None, SkillOptions::default()).unwrap();
    let report = uninstall(Host::Claude, &scratch.0, None, SkillOptions::default()).unwrap();
    assert_eq!(
        report.changed_files,
        vec![scratch.0.join("skills/tokenimum/SKILL.md")]
    );
    assert!(!scratch.0.join("skills/tokenimum").exists());
}

#[test]
fn uninstall_dry_run_keeps_file() {
    let scratch = Scratch::new();
    install(Host::Claude, &scratch.0, None, SkillOptions::default()).unwrap();
    let report = uninstall(
        Host::Claude,
        &scratch.0,
        None,
        SkillOptions {
            dry_run: true,
            ..SkillOptions::default()
        },
    )
    .unwrap();
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
    let report = uninstall(Host::Claude, &scratch.0, None, SkillOptions::default()).unwrap();
    assert!(report.changed_files.is_empty(), "unexpected report");
    assert!(path.is_file());
}

#[test]
fn uninstall_absent_and_unsupported_are_noops() {
    let scratch = Scratch::new();
    for host in [Host::Claude, Host::Codex] {
        let report = uninstall(host, &scratch.0, None, SkillOptions::default()).unwrap();
        assert_eq!(report.changed_files, Vec::<PathBuf>::new());
    }
}

#[test]
fn install_project_scope_refuses_tracked_skill_without_opt_in() {
    let scratch = Scratch::new();
    let home = scratch.0.join("home");
    let repo = scratch.0.join("repo");
    let path = repo.join(".claude/skills/tokenimum/SKILL.md");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    git(&repo, &["init", "-q"]);
    fs::write(&path, render(Host::Claude)).unwrap();
    git(&repo, &["add", "."]);
    git(
        &repo,
        &[
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@example.com",
            "commit",
            "-qm",
            "x",
        ],
    );
    let stale = fs::read_to_string(&path)
        .unwrap()
        .replace("host=claude", "host=old");
    fs::write(&path, &stale).unwrap();
    let err = install(Host::Claude, &home, Some(&repo), SkillOptions::default()).unwrap_err();
    assert!(matches!(err, AdapterError::UnsafeTarget { .. }), "{err:?}");
    assert_eq!(fs::read_to_string(&path).unwrap(), stale);
    let err = uninstall(Host::Claude, &home, Some(&repo), SkillOptions::default()).unwrap_err();
    assert!(matches!(err, AdapterError::UnsafeTarget { .. }), "{err:?}");
    let allow = SkillOptions {
        allow_tracked: true,
        ..SkillOptions::default()
    };
    install(Host::Claude, &home, Some(&repo), allow).unwrap();
    assert_eq!(fs::read_to_string(&path).unwrap(), render(Host::Claude));
}

#[cfg(unix)]
#[test]
fn symlinked_skill_file_is_refused_by_install_and_uninstall() {
    let scratch = Scratch::new();
    let dir = scratch.0.join("skills/tokenimum");
    fs::create_dir_all(&dir).unwrap();
    let real = scratch.0.join("skills/real.md");
    fs::write(&real, render(Host::Claude)).unwrap();
    std::os::unix::fs::symlink(&real, dir.join("SKILL.md")).unwrap();
    let err = install(Host::Claude, &scratch.0, None, SkillOptions::default()).unwrap_err();
    assert!(matches!(err, AdapterError::UnsafeTarget { .. }), "{err:?}");
    let err = uninstall(Host::Claude, &scratch.0, None, SkillOptions::default()).unwrap_err();
    assert!(matches!(err, AdapterError::UnsafeTarget { .. }), "{err:?}");
    assert!(real.is_file());
    assert!(
        dir.join("SKILL.md")
            .symlink_metadata()
            .unwrap()
            .file_type()
            .is_symlink()
    );
}

#[test]
fn uninstall_keeps_directory_that_holds_other_files() {
    let scratch = Scratch::new();
    install(Host::Claude, &scratch.0, None, SkillOptions::default()).unwrap();
    let dir = scratch.0.join("skills/tokenimum");
    fs::write(dir.join("notes.md"), "mine").unwrap();
    uninstall(Host::Claude, &scratch.0, None, SkillOptions::default()).unwrap();
    assert!(!dir.join("SKILL.md").exists());
    assert!(dir.join("notes.md").is_file());
    assert!(scratch.0.join("skills").is_dir());
}
