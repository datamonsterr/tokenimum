use std::process::{Command, Output};
use tkm_config::{paths::Paths, repo::repo_identity};
use tkm_core::model::{RepoKey, Timestamp};
use tkm_store::{Repo, Store};

pub struct Fixture {
    pub home: tempfile::TempDir,
    pub repo: RepoKey,
    pub db: std::path::PathBuf,
}

impl Fixture {
    pub fn new() -> Self {
        let home = tempfile::tempdir().unwrap();
        let paths = Paths::from_dirs(
            home.path(),
            Some(&home.path().join("data")),
            Some(&home.path().join("config")),
            Some(&home.path().join("run")),
        );
        let repo = repo_identity(home.path(), "", Some(home.path())).key;
        let store = Store::open(&paths.db).unwrap();
        store
            .upsert_repo(&Repo {
                repo_key: repo.clone(),
                display_name: "fixture".into(),
                first_seen: Timestamp(1),
                last_seen: Timestamp(1),
                delegated_runs: 0,
            })
            .unwrap();
        Self {
            home,
            repo,
            db: paths.db,
        }
    }

    pub fn store(&self) -> Store {
        Store::open(&self.db).unwrap()
    }

    pub fn run(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_tkm"))
            .args(args)
            .env_clear()
            .env("HOME", self.home.path())
            .env("PATH", self.home.path().join("bin"))
            .env("XDG_DATA_HOME", self.home.path().join("data"))
            .env("XDG_CONFIG_HOME", self.home.path().join("config"))
            .env("XDG_CACHE_HOME", self.home.path().join("cache"))
            .env("XDG_RUNTIME_DIR", self.home.path().join("run"))
            .current_dir(self.home.path())
            .output()
            .unwrap()
    }
}
