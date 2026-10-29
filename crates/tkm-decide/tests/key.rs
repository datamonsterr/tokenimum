#![forbid(unsafe_code)]
#![allow(clippy::unwrap_used, clippy::expect_used)]
use std::{
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};
use tkm_decide::key::{KeyError, default_key_file, load_key};
static NEXT: AtomicU64 = AtomicU64::new(0);
fn dir() -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "tkm-decide-key-{}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&path).unwrap();
    path
}
#[test]
fn environment_key_wins_without_reading_either_file_and_debug_is_redacted() {
    let key = load_key(
        Some("fake-env-secret".into()),
        std::path::Path::new("missing"),
        Some(std::path::Path::new("also-missing")),
    )
    .unwrap();
    assert_eq!(key.expose_secret(), "fake-env-secret");
    assert!(!format!("{key:?}").contains("fake-env-secret"));
}
#[test]
fn default_file_is_below_home_config_directory() {
    assert_eq!(
        default_key_file(std::path::Path::new("/fake-home")),
        PathBuf::from("/fake-home/.config/tokenimum/openrouter.env")
    );
}
#[cfg(unix)]
#[test]
fn mode_0644_is_rejected_and_0600_is_accepted() {
    use std::os::unix::fs::PermissionsExt;
    let dir = dir();
    let path = dir.join("openrouter.env");
    std::fs::write(&path, "OPENROUTER_API_KEY=fake-file-secret\n").unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
    assert!(matches!(
        load_key(None, &path, None),
        Err(KeyError::InsecurePermissions(_))
    ));
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
    assert_eq!(
        load_key(None, &path, None).unwrap().expose_secret(),
        "fake-file-secret"
    );
    std::fs::remove_dir_all(dir).unwrap();
}
#[cfg(unix)]
#[test]
fn reuse_path_is_read_in_place_never_copied_and_supports_export_quotes_comments() {
    use std::os::unix::fs::PermissionsExt;
    let dir = dir();
    let reuse = dir.join("reused.env");
    let destination = dir.join("not-created.env");
    let text = "# fake values only\nOTHER=ignored\nexport OPENROUTER_API_KEY=\"fake-reused-secret\" # comment\n";
    std::fs::write(&reuse, text).unwrap();
    std::fs::set_permissions(&reuse, std::fs::Permissions::from_mode(0o600)).unwrap();
    assert_eq!(
        load_key(None, &destination, Some(&reuse))
            .unwrap()
            .expose_secret(),
        "fake-reused-secret"
    );
    assert!(!destination.exists());
    assert_eq!(std::fs::read_to_string(&reuse).unwrap(), text);
    std::fs::remove_dir_all(dir).unwrap();
}
#[cfg(unix)]
#[test]
fn quoted_hash_is_preserved_and_empty_key_is_rejected() {
    use std::os::unix::fs::PermissionsExt;
    let dir = dir();
    let path = dir.join("key.env");
    std::fs::write(&path, "OPENROUTER_API_KEY='fake#quoted' # comment\n").unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
    assert_eq!(
        load_key(None, &path, None).unwrap().expose_secret(),
        "fake#quoted"
    );
    std::fs::write(&path, "OPENROUTER_API_KEY=\n").unwrap();
    assert!(matches!(
        load_key(None, &path, None),
        Err(KeyError::Missing)
    ));
    std::fs::remove_dir_all(dir).unwrap();
}
