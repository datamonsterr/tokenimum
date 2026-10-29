#![forbid(unsafe_code)]
#![allow(clippy::unwrap_used, clippy::expect_used)]
use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};
use tkm_config::{
    paths::{ERRORS_LOG_MAX_BYTES, ErrorLog, Paths, secure_socket},
    repo::{IdentitySource, find_repo_root, is_git_entry, normalize_remote, repo_identity},
};
static NEXT: AtomicU64 = AtomicU64::new(0);
struct Temp(PathBuf);
impl Temp {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "tkm-config-{}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn checkout(root: &std::path::Path, remote: Option<&str>) {
    fs::create_dir_all(root.join(".git")).unwrap();
    fs::write(root.join(".git/HEAD"), "ref: refs/heads/main\n").unwrap();
    if let Some(url) = remote {
        fs::write(
            root.join(".git/config"),
            format!("[remote \"origin\"]\n url = {url}\n"),
        )
        .unwrap();
    }
}
fn worktree(main: &std::path::Path, root: &std::path::Path, name: &str) {
    let gitdir = main.join(".git/worktrees").join(name);
    fs::create_dir_all(&gitdir).unwrap();
    fs::create_dir_all(root).unwrap();
    fs::write(gitdir.join("HEAD"), "ref: refs/heads/main\n").unwrap();
    fs::write(gitdir.join("commondir"), "../..\n").unwrap();
    fs::write(root.join(".git"), format!("gitdir: {}\n", gitdir.display())).unwrap();
}
#[test]
fn main_two_worktrees_and_https_clone_share_remote_key() {
    let t = Temp::new();
    let main = t.0.join("main");
    let a = t.0.join("a");
    let b = t.0.join("b");
    let clone = t.0.join("clone");
    checkout(&main, Some("git@github.com:Owner/Repo.git"));
    worktree(&main, &a, "a");
    worktree(&main, &b, "b");
    checkout(&clone, Some("https://github.com/owner/repo"));
    for p in [&main, &a, &b, &clone] {
        assert_eq!(repo_identity(p, "", None).key.0, "github.com/owner/repo");
    }
}
#[test]
fn no_remote_worktree_uses_main_path_and_walk_honors_ceilings() {
    let t = Temp::new();
    let main = t.0.join("main");
    let wt = t.0.join("wt");
    checkout(&main, None);
    worktree(&main, &wt, "wt");
    let sub = main.join("a/b");
    fs::create_dir_all(&sub).unwrap();
    assert_eq!(find_repo_root(&sub, "", None), Some(main.clone()));
    assert_eq!(
        repo_identity(&wt, "", None).key.0,
        format!("path:{}", main.canonicalize().unwrap().display())
    );
    assert_eq!(find_repo_root(&sub, &main.to_string_lossy(), None), None);
    assert_eq!(find_repo_root(&sub, "", Some(&main)), None);
    let empty = t.0.join("empty/.git");
    fs::create_dir_all(&empty).unwrap();
    assert!(!is_git_entry(&empty));
}
#[test]
fn normalize_remote_variants() {
    for input in [
        "git@github.com:Owner/Repo.git",
        "https://github.com/owner/repo/",
        "ssh://git@github.com/Owner/Repo.git",
    ] {
        assert_eq!(normalize_remote(input), "github.com/owner/repo");
    }
}
#[test]
fn errors_log_rotates_before_next_write() {
    let t = Temp::new();
    let path = t.0.join("errors.log");
    fs::write(&path, "12345").unwrap();
    fs::write(t.0.join("errors.log.1"), "old").unwrap();
    ErrorLog::new(path.clone(), 5).append("new").unwrap();
    assert_eq!(
        fs::read_to_string(t.0.join("errors.log.1")).unwrap(),
        "12345"
    );
    assert!(fs::read_to_string(path).unwrap().contains("new"));
}
#[cfg(unix)]
#[test]
fn runtime_directory_and_socket_have_private_modes() {
    use std::os::unix::fs::PermissionsExt;
    let t = Temp::new();
    let paths = Paths::from_dirs(&t.0, None, None, Some(&t.0));
    paths.ensure_runtime_dir().unwrap();
    fs::write(&paths.socket, "").unwrap();
    secure_socket(&paths.socket).unwrap();
    assert_eq!(
        fs::metadata(paths.socket.parent().unwrap())
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o700
    );
    assert_eq!(
        fs::metadata(paths.socket).unwrap().permissions().mode() & 0o777,
        0o600
    );
}

#[test]
fn repository_identity_source_is_typed() {
    let t = Temp::new();
    let main = t.0.join("main");
    checkout(&main, Some("https://github.com/owner/repo"));
    assert_eq!(repo_identity(&main, "", None).how, IdentitySource::Remote);
    assert_eq!(IdentitySource::Remote.as_str(), "remote");
    fs::remove_file(main.join(".git/config")).unwrap();
    assert_eq!(repo_identity(&main, "", None).how, IdentitySource::Path);
    assert_eq!(IdentitySource::Path.as_str(), "path");
    assert_eq!(repo_identity(&t.0, "", None).how, IdentitySource::Dir);
    assert_eq!(IdentitySource::Dir.as_str(), "dir");
}
#[test]
fn errors_log_default_limit_is_5_mib() {
    let t = Temp::new();
    let paths = Paths::from_dirs(&t.0, None, None, None);
    fs::create_dir_all(&paths.data_dir).unwrap();
    assert_eq!(ERRORS_LOG_MAX_BYTES, 5 * 1024 * 1024);
    fs::write(
        &paths.errors_log,
        vec![b'x'; usize::try_from(ERRORS_LOG_MAX_BYTES).unwrap()],
    )
    .unwrap();
    ErrorLog::for_paths(&paths).append("next").unwrap();
    assert_eq!(
        fs::metadata(paths.data_dir.join("errors.log.1"))
            .unwrap()
            .len(),
        ERRORS_LOG_MAX_BYTES
    );
    assert!(
        fs::read_to_string(&paths.errors_log)
            .unwrap()
            .contains("next")
    );
}
