#![forbid(unsafe_code)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::pedantic)]
mod support_ops;
use tkm_cli::{
    cli::{PruneApplyArgs, PruneArgs, PruneCommand},
    cmd::prune::run_with_clock,
    ctx::Ctx,
};
use tkm_core::model::{Host, Timestamp};
use tkmd::clock::ManualClock;

#[test]
fn mocked_clock_applies_at_idle_boundary_or_ready_deadline() {
    for deadline in [false, true] {
        let s = support_ops::Sandbox::new();
        let paths = tkm_config::paths::Paths::from_dirs(
            s.home.path(),
            Some(&s.home.path().join("data")),
            Some(&s.home.path().join("config")),
            Some(&s.home.path().join("run")),
        );
        let store = tkm_store::Store::open(&paths.db).unwrap();
        let repo = tkm_config::repo::repo_identity(&s.repo, "", Some(s.home.path()));
        let ready = 10_000;
        let boundary = if deadline {
            ready + 86_400_000
        } else {
            ready + 3_600_000
        };
        store
            .upsert_repo(&tkm_store::Repo {
                repo_key: repo.key.clone(),
                display_name: "test".into(),
                first_seen: Timestamp(0),
                last_seen: Timestamp(if deadline { boundary - 1 } else { ready }),
                delegated_runs: 0,
            })
            .unwrap();
        for _ in 0..5 {
            store
                .record_tool_unused_session(&repo.key, Host::Claude, "mcp__example__unused")
                .unwrap();
        }
        store
            .refresh_prune_readiness(
                &repo.key,
                Host::Claude,
                &Default::default(),
                Timestamp(ready),
            )
            .unwrap();
        let clock = ManualClock::new(u64::try_from(boundary - 1).unwrap());
        let mut out = Vec::new();
        let mut err = Vec::new();
        let mut ctx = Ctx::new(paths, s.repo.clone(), repo, &mut out, &mut err);
        ctx.env = vec![
            ("HOME".into(), s.home.path().display().to_string()),
            (
                "PATH".into(),
                s.home.path().join("bin").display().to_string(),
            ),
        ];
        let args = PruneArgs {
            command: Some(PruneCommand::Apply(PruneApplyArgs::default())),
        };
        // Use a real git repository because in-process writers use the process PATH.
        std::fs::remove_dir_all(s.repo.join(".git")).unwrap();
        assert!(
            std::process::Command::new("git")
                .args(["init", "--quiet"])
                .arg(&s.repo)
                .status()
                .unwrap()
                .success()
        );
        assert!(run_with_clock(&mut ctx, &args, &clock).is_err());
        assert!(!s.repo.join(".claude/settings.local.json").exists());
        clock.set(u64::try_from(boundary).unwrap());
        assert!(
            run_with_clock(&mut ctx, &args, &clock).unwrap() == std::process::ExitCode::SUCCESS
        );
        assert!(s.repo.join(".claude/settings.local.json").exists());
    }
}
