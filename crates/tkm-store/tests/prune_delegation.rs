#![forbid(unsafe_code)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::pedantic)]
use tkm_core::model::{Host, RepoKey, Timestamp};
use tkm_store::{Repo, SessionKey, Store};

#[test]
fn delegated_history_is_idempotent_scoped_and_queries_exact_thirty_day_window() {
    let store = Store::open_in_memory().unwrap();
    let repo = RepoKey("r".into());
    let other = RepoKey("other".into());
    for key in [&repo, &other] {
        store
            .upsert_repo(&Repo {
                repo_key: key.clone(),
                display_name: "test".into(),
                first_seen: Timestamp(0),
                last_seen: Timestamp(0),
                delegated_runs: 42,
            })
            .unwrap();
    }
    let key = SessionKey {
        host: Host::Claude,
        agent_home: "/test/home".into(),
        session_id: "run-1".into(),
    };
    let at = Timestamp(1000);
    store.record_delegated_run(&repo, &key, at).unwrap();
    store
        .record_delegated_run(&repo, &key, Timestamp(2000))
        .unwrap();
    assert_eq!(store.get_repo(&repo).unwrap().unwrap().delegated_runs, 43);
    assert_eq!(
        store
            .last_delegated_run_within(&repo, Timestamp(1000 + 2_592_000_000))
            .unwrap(),
        Some(at)
    );
    assert_eq!(
        store
            .last_delegated_run_within(&repo, Timestamp(1001 + 2_592_000_000))
            .unwrap(),
        None
    );
    assert_eq!(
        store
            .last_delegated_run_within(&repo, Timestamp(999))
            .unwrap(),
        None
    );
    assert_eq!(
        store
            .last_delegated_run_within(&other, Timestamp(1000))
            .unwrap(),
        None
    );
    let newer = SessionKey {
        session_id: "run-2".into(),
        ..key
    };
    store
        .record_delegated_run(&repo, &newer, Timestamp(3000))
        .unwrap();
    assert_eq!(
        store
            .last_delegated_run_within(&repo, Timestamp(4000))
            .unwrap(),
        Some(Timestamp(3000))
    );
}
