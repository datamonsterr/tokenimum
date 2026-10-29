#![forbid(unsafe_code)]
#![allow(clippy::float_cmp, clippy::unwrap_used)]
use tkm_core::{
    ledger::{SavingsSession, estimate_savings},
    model::{Host, RepoKey, Timestamp},
};
#[test]
fn savings_groups_and_confidence() {
    let repo = RepoKey("repo".into());
    let make = |started, shadow, cost| SavingsSession {
        repo: repo.clone(),
        host: Host::Claude,
        started: Timestamp(started),
        shadow,
        cost,
    };
    let mut sessions = vec![
        make(1, false, 10.0),
        make(4, true, 14.0),
        make(2, false, 4.0),
        make(3, false, 8.0),
    ];
    sessions.push(SavingsSession {
        repo: RepoKey("other".into()),
        ..make(1, false, 999.0)
    });
    let result = estimate_savings(&sessions, &repo, Host::Claude, Some(Timestamp(2)));
    assert_eq!((result.baseline_count, result.treatment_count), (2, 2));
    assert_eq!(
        (
            result.baseline_mean,
            result.treatment_mean,
            result.difference
        ),
        (Some(12.0), Some(6.0), Some(6.0))
    );
    let band = result.band.unwrap();
    assert!((band.lower - (6.0 - 1.96 * 8.0_f64.sqrt())).abs() < 1e-10);
    assert!((band.upper - (6.0 + 1.96 * 8.0_f64.sqrt())).abs() < 1e-10);
    let insufficient = estimate_savings(&sessions[..1], &repo, Host::Claude, Some(Timestamp(2)));
    assert_eq!(
        (
            insufficient.baseline_count,
            insufficient.treatment_count,
            insufficient.band
        ),
        (1, 0, None)
    );
    assert_eq!(
        estimate_savings(&sessions, &repo, Host::Claude, None).treatment_count,
        0
    );
}

#[test]
fn savings_classification_table() {
    let repo = RepoKey("repo".into());
    for (started, shadow, host, first, baseline_count, treatment_count) in [
        (1, false, Host::Claude, Some(Timestamp(2)), 1, 0),
        (2, false, Host::Claude, Some(Timestamp(2)), 0, 1),
        (3, true, Host::Claude, Some(Timestamp(2)), 1, 0),
        (3, false, Host::Claude, None, 1, 0),
        (3, false, Host::Codex, Some(Timestamp(2)), 0, 0),
    ] {
        let session = SavingsSession {
            repo: repo.clone(),
            host,
            started: Timestamp(started),
            shadow,
            cost: 1.0,
        };
        let estimate = estimate_savings(&[session], &repo, Host::Claude, first);
        assert_eq!(
            (estimate.baseline_count, estimate.treatment_count),
            (baseline_count, treatment_count)
        );
        assert_eq!(estimate.band, None);
    }
}
