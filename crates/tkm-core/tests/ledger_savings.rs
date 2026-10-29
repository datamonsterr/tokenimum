#![forbid(unsafe_code)]
#![allow(clippy::float_cmp, clippy::unwrap_used)]
use tkm_core::{
    ledger::{SavingsSession, checked_estimate_savings, estimate_savings},
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
    let result = estimate_savings(&sessions, &repo, Host::Claude, Some(Timestamp(2))).unwrap();
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
    assert!((band.lower - (-6.169_739_69)).abs() < 0.000_01);
    assert!((band.upper - (18.169_739_69)).abs() < 0.000_01);
    let insufficient =
        estimate_savings(&sessions[..1], &repo, Host::Claude, Some(Timestamp(2))).unwrap();
    assert_eq!(
        (
            insufficient.baseline_count,
            insufficient.treatment_count,
            insufficient.band
        ),
        (1, 0, None)
    );
    assert_eq!(
        estimate_savings(&sessions, &repo, Host::Claude, None)
            .unwrap()
            .treatment_count,
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
        let estimate = estimate_savings(&[session], &repo, Host::Claude, first).unwrap();
        assert_eq!(
            (estimate.baseline_count, estimate.treatment_count),
            (baseline_count, treatment_count)
        );
        assert_eq!(estimate.band, None);
    }
}

#[test]
fn savings_rejects_invalid_costs_and_overflow() {
    let repo = RepoKey("repo".into());
    for cost in [-1.0, f64::NAN, f64::INFINITY] {
        let session = SavingsSession {
            repo: repo.clone(),
            host: Host::Claude,
            started: Timestamp(0),
            shadow: true,
            cost,
        };
        assert!(checked_estimate_savings(&[session], &repo, Host::Claude, None).is_none());
    }
    let s = SavingsSession {
        repo: repo.clone(),
        host: Host::Claude,
        started: Timestamp(0),
        shadow: true,
        cost: f64::MAX,
    };
    assert!(checked_estimate_savings(&[s.clone(), s], &repo, Host::Claude, None).is_none());
}

#[test]
fn welch_unequal_variance_and_constant_groups() {
    let repo = RepoKey("example".into());
    let sample = |cost, shadow| SavingsSession {
        repo: repo.clone(),
        host: Host::Claude,
        started: Timestamp(0),
        shadow,
        cost,
    };
    let data = [
        sample(10.0, true),
        sample(14.0, true),
        sample(6.0, false),
        sample(6.0, false),
    ];
    let band = estimate_savings(&data, &repo, Host::Claude, Some(Timestamp(0)))
        .unwrap()
        .band
        .unwrap();
    // Independent Student t(0.975, 1) = 12.706_205, standard error = 2.
    assert!((band.lower - (-19.412_41)).abs() < 0.000_01);
    assert!((band.upper - 31.412_41).abs() < 0.000_01);
    let flat = [
        sample(10.0, true),
        sample(10.0, true),
        sample(6.0, false),
        sample(6.0, false),
    ];
    let band = estimate_savings(&flat, &repo, Host::Claude, Some(Timestamp(0)))
        .unwrap()
        .band
        .unwrap();
    assert_eq!((band.lower, band.upper), (4.0, 4.0));
}
