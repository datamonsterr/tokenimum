#![forbid(unsafe_code)]
#![allow(clippy::unwrap_used, clippy::expect_used)]
mod support_report;
use support_report::Fixture;
use tkm_core::model::{RepoKey, Timestamp};
use tkm_store::Repo;

#[test]
fn status_daemon_down_uses_db() {
    let f = Fixture::new();
    let out = f.run(&["status"]);
    assert!(out.status.success(), "{out:?}");
    let text = String::from_utf8(out.stdout).unwrap();
    assert!(text.contains("down (read-only DB)"));
    assert!(text.contains(&f.repo.0));
}
#[test]
fn status_all_lists_repos() {
    let f = Fixture::new();
    f.store()
        .upsert_repo(&Repo {
            repo_key: RepoKey("fixture/other".into()),
            display_name: "other".into(),
            first_seen: Timestamp(1),
            last_seen: Timestamp(1),
            delegated_runs: 0,
        })
        .unwrap();
    let out = f.run(&["status", "--all"]);
    assert!(out.status.success(), "{out:?}");
    let text = String::from_utf8(out.stdout).unwrap();
    assert!(text.contains(&f.repo.0));
    assert!(text.contains("fixture/other"));
}
