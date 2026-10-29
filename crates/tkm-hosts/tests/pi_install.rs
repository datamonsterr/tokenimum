#![forbid(unsafe_code)]
#![allow(clippy::unwrap_used, clippy::expect_used)]
use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};
use tkm_hosts::{
    AdapterError, HostAdapter, InstallContext,
    pi::PiAdapter,
    shim::{ShimStatus, render, verify_shim},
};
use tkm_proto::Host;

const VERSION: &str = env!("CARGO_PKG_VERSION");

struct Scratch(PathBuf);
impl Scratch {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "tkm-pi-install-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }
    fn home(&self) -> PathBuf {
        self.0.join("home/.pi/agent")
    }
    fn repo(&self) -> PathBuf {
        self.0.join("repo")
    }
    fn user(&self) -> InstallContext {
        InstallContext {
            agent_home: self.home(),
            ..InstallContext::default()
        }
    }
    fn project(&self) -> InstallContext {
        InstallContext {
            agent_home: self.home(),
            repo_root: Some(self.repo()),
            dry_run: false,
        }
    }
    fn shim(&self) -> PathBuf {
        self.home().join("extensions/tokenimum.ts")
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn expected() -> Vec<u8> {
    render(Host::Pi, VERSION).unwrap()
}

#[test]
fn user_install_writes_the_verified_shim_under_extensions() {
    let scratch = Scratch::new();
    let report = PiAdapter::new().install(&scratch.user()).unwrap();
    assert_eq!(report.changed_files, vec![scratch.shim()]);
    assert_eq!(report.instructions.len(), 1);
    assert!(report.instructions[0].contains("/reload"));
    assert!(report.instructions[0].contains("project trust"));
    assert!(!report.dry_run);
    let bytes = fs::read(scratch.shim()).unwrap();
    assert_eq!(bytes, expected());
    assert_eq!(verify_shim(&bytes), ShimStatus::Ok);
}

#[test]
fn project_install_prints_the_trust_step_and_writes_under_the_repository_only() {
    let scratch = Scratch::new();
    let report = PiAdapter::new().install(&scratch.project()).unwrap();
    let path = scratch.repo().join(".pi/extensions/tokenimum.ts");
    assert_eq!(report.changed_files, vec![path.clone()]);
    assert_eq!(fs::read(path).unwrap(), expected());
    assert!(!scratch.home().exists());
}

#[test]
fn second_install_changes_nothing() {
    let scratch = Scratch::new();
    let adapter = PiAdapter::new();
    adapter.install(&scratch.user()).unwrap();
    let before = fs::metadata(scratch.shim()).unwrap().modified().unwrap();
    let report = adapter.install(&scratch.user()).unwrap();
    assert_eq!(report.changed_files.len(), 0);
    assert_eq!(fs::read(scratch.shim()).unwrap(), expected());
    assert_eq!(
        fs::metadata(scratch.shim()).unwrap().modified().unwrap(),
        before
    );
}

#[test]
fn install_replaces_an_older_untouched_shim() {
    let scratch = Scratch::new();
    fs::create_dir_all(scratch.shim().parent().unwrap()).unwrap();
    fs::write(scratch.shim(), render(Host::Pi, "0.0.0-old").unwrap()).unwrap();
    let report = PiAdapter::new().install(&scratch.user()).unwrap();
    assert_eq!(report.changed_files, vec![scratch.shim()]);
    assert_eq!(fs::read(scratch.shim()).unwrap(), expected());
}

#[test]
fn dry_run_writes_nothing() {
    let scratch = Scratch::new();
    let context = InstallContext {
        dry_run: true,
        ..scratch.user()
    };
    let report = PiAdapter::new().install(&context).unwrap();
    assert!(report.dry_run);
    assert_eq!(report.changed_files, vec![scratch.shim()]);
    assert!(!scratch.0.join("home").exists());
    PiAdapter::new().install(&scratch.user()).unwrap();
    let report = PiAdapter::new().uninstall(&context).unwrap();
    assert_eq!(report.changed_files, vec![scratch.shim()]);
    assert!(scratch.shim().exists());
}

#[test]
fn uninstall_removes_only_the_verified_shim() {
    let scratch = Scratch::new();
    let adapter = PiAdapter::new();
    adapter.install(&scratch.user()).unwrap();
    let report = adapter.uninstall(&scratch.user()).unwrap();
    assert_eq!(report.changed_files, vec![scratch.shim()]);
    assert_eq!(report.instructions.len(), 0);
    assert!(!scratch.shim().exists());
    let again = adapter.uninstall(&scratch.user()).unwrap();
    assert_eq!(again.changed_files.len(), 0);
}

#[test]
fn tampered_shim_is_kept_by_uninstall_and_install() {
    let scratch = Scratch::new();
    let adapter = PiAdapter::new();
    adapter.install(&scratch.user()).unwrap();
    let mut bytes = fs::read(scratch.shim()).unwrap();
    bytes.extend_from_slice(b"// local edit\n");
    fs::write(scratch.shim(), &bytes).unwrap();
    assert!(matches!(
        adapter.uninstall(&scratch.user()),
        Err(AdapterError::UnsafeTarget { .. })
    ));
    assert!(matches!(
        adapter.install(&scratch.user()),
        Err(AdapterError::UnsafeTarget { .. })
    ));
    assert_eq!(fs::read(scratch.shim()).unwrap(), bytes);
}

#[test]
fn foreign_file_is_never_overwritten_or_removed() {
    let scratch = Scratch::new();
    let adapter = PiAdapter::new();
    fs::create_dir_all(scratch.shim().parent().unwrap()).unwrap();
    fs::write(scratch.shim(), "export const Mine = async () => ({})\n").unwrap();
    assert!(matches!(
        adapter.install(&scratch.user()),
        Err(AdapterError::UnsafeTarget { .. })
    ));
    let report = adapter.uninstall(&scratch.user()).unwrap();
    assert_eq!(report.changed_files.len(), 0);
    assert_eq!(
        fs::read_to_string(scratch.shim()).unwrap(),
        "export const Mine = async () => ({})\n"
    );
}

#[cfg(unix)]
#[test]
fn symlinked_plugin_directory_outside_home_is_refused() {
    let scratch = Scratch::new();
    let outside = scratch.0.join("outside");
    fs::create_dir_all(&outside).unwrap();
    fs::create_dir_all(scratch.home()).unwrap();
    std::os::unix::fs::symlink(&outside, scratch.home().join("extensions")).unwrap();
    let adapter = PiAdapter::new();
    assert!(matches!(
        adapter.install(&scratch.user()),
        Err(AdapterError::UnsafeTarget { .. })
    ));
    assert_eq!(fs::read_dir(&outside).unwrap().count(), 0);
}

#[cfg(unix)]
#[test]
fn symlinked_shim_file_outside_home_is_refused_and_left_alone() {
    let scratch = Scratch::new();
    let outside = scratch.0.join("outside.ts");
    fs::write(&outside, expected()).unwrap();
    fs::create_dir_all(scratch.shim().parent().unwrap()).unwrap();
    std::os::unix::fs::symlink(&outside, scratch.shim()).unwrap();
    let adapter = PiAdapter::new();
    assert!(matches!(
        adapter.install(&scratch.user()),
        Err(AdapterError::UnsafeTarget { .. })
    ));
    assert!(matches!(
        adapter.uninstall(&scratch.user()),
        Err(AdapterError::UnsafeTarget { .. })
    ));
    assert!(outside.exists());
    assert!(
        scratch
            .shim()
            .symlink_metadata()
            .unwrap()
            .file_type()
            .is_symlink()
    );
}

#[cfg(unix)]
#[test]
fn shim_that_is_a_symlink_inside_the_root_is_refused_by_install_and_uninstall() {
    let scratch = Scratch::new();
    let real = scratch.home().join("extensions/real.ts");
    fs::create_dir_all(real.parent().unwrap()).unwrap();
    fs::write(&real, expected()).unwrap();
    std::os::unix::fs::symlink(&real, scratch.shim()).unwrap();
    let adapter = PiAdapter::new();
    assert!(matches!(
        adapter.install(&scratch.user()),
        Err(AdapterError::UnsafeTarget { .. })
    ));
    assert!(matches!(
        adapter.uninstall(&scratch.user()),
        Err(AdapterError::UnsafeTarget { .. })
    ));
    assert_eq!(fs::read(&real).unwrap(), expected());
    assert!(
        scratch
            .shim()
            .symlink_metadata()
            .unwrap()
            .file_type()
            .is_symlink()
    );
}
