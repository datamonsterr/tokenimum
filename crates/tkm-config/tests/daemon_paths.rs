#![forbid(unsafe_code)]
#![allow(clippy::unwrap_used, clippy::expect_used)]
//! One daemon per data directory: socket resolution and the data-directory lock.
use std::{
    fs,
    os::unix::fs::{MetadataExt, PermissionsExt},
    path::Path,
};
use tkm_config::paths::{DAEMON_LOCK_NAME, Paths, resolve_runtime_dir};

fn my_uid() -> u32 {
    fs::metadata("/proc/self")
        .or_else(|_| fs::metadata(std::env::temp_dir()))
        .unwrap()
        .uid()
}

fn user_root(tmp: &Path, uid: u32, mode: u32) -> std::path::PathBuf {
    let root = tmp.join("run-user");
    let dir = root.join(uid.to_string());
    fs::create_dir_all(&dir).unwrap();
    fs::set_permissions(&dir, fs::Permissions::from_mode(mode)).unwrap();
    root
}

#[test]
fn unset_xdg_runtime_dir_falls_back_to_the_login_runtime_dir() {
    let tmp = tempfile::tempdir().unwrap();
    let uid = my_uid();
    let root = user_root(tmp.path(), uid, 0o700);
    let login = root.join(uid.to_string());

    let with_env = resolve_runtime_dir(Some(&login), &root, Some(uid));
    let without_env = resolve_runtime_dir(None, &root, Some(uid));
    assert_eq!(with_env.as_deref(), Some(login.as_path()));
    assert_eq!(without_env, with_env);

    let home = tmp.path().join("home");
    let a = Paths::from_dirs(&home, None, None, with_env.as_deref());
    let b = Paths::from_dirs(&home, None, None, without_env.as_deref());
    assert_eq!(a.socket, b.socket);
    assert_eq!(a.socket, login.join("tokenimum/tkmd.sock"));
}

#[test]
fn login_runtime_dir_is_used_only_when_private() {
    let tmp = tempfile::tempdir().unwrap();
    let uid = my_uid();
    let root = user_root(tmp.path(), uid, 0o755);
    assert_eq!(resolve_runtime_dir(None, &root, Some(uid)), None);
    // Another user's directory is never borrowed.
    assert_eq!(
        resolve_runtime_dir(None, &root, Some(uid.wrapping_add(1))),
        None
    );
    assert_eq!(
        resolve_runtime_dir(None, &tmp.path().join("missing"), Some(uid)),
        None
    );
    assert_eq!(resolve_runtime_dir(None, &root, None), None);
}

#[test]
fn relative_xdg_runtime_dir_is_ignored() {
    let tmp = tempfile::tempdir().unwrap();
    let uid = my_uid();
    let root = user_root(tmp.path(), uid, 0o700);
    assert_eq!(
        resolve_runtime_dir(Some(Path::new("relative/run")), &root, Some(uid)),
        Some(root.join(uid.to_string()))
    );
}

#[test]
fn daemon_lock_lives_in_the_data_dir_not_the_runtime_dir() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    let a = Paths::from_dirs(&home, None, None, Some(&tmp.path().join("run-a")));
    let b = Paths::from_dirs(&home, None, None, None);
    assert_ne!(a.socket, b.socket);
    assert_eq!(a.daemon_lock, b.daemon_lock);
    assert_eq!(a.daemon_lock, a.data_dir.join(DAEMON_LOCK_NAME));
}

#[test]
fn second_lock_on_the_same_data_dir_is_refused_whatever_the_runtime_dir() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    let first = Paths::from_dirs(&home, None, None, Some(&tmp.path().join("run-a")));
    let second = Paths::from_dirs(&home, None, None, None);

    assert!(!second.daemon_running().unwrap());
    let held = first.lock_daemon().unwrap().expect("first lock");
    assert!(second.lock_daemon().unwrap().is_none());
    assert!(second.daemon_running().unwrap());
    drop(held);
    assert!(!second.daemon_running().unwrap());
    assert!(second.lock_daemon().unwrap().is_some());
}
