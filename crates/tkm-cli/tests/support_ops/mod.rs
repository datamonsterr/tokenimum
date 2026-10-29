#![allow(dead_code)]
use std::{
    path::PathBuf,
    process::{Command, Output},
};

pub struct Sandbox {
    pub home: tempfile::TempDir,
    pub repo: PathBuf,
}
impl Sandbox {
    pub fn new() -> Self {
        let home = tempfile::tempdir().unwrap();
        let repo = home.path().join("repo");
        std::fs::create_dir_all(repo.join(".git")).unwrap();
        std::fs::write(repo.join(".git/HEAD"), "ref: refs/heads/main\n").unwrap();
        std::fs::create_dir_all(home.path().join("bin")).unwrap();
        std::fs::create_dir_all(repo.join(".git/info")).unwrap();
        let git = home.path().join("bin/git");
        std::fs::write(&git, "#!/bin/sh\ncase \"$1 $2\" in\n  'ls-files --error-unmatch') exit 1 ;;\n  'rev-parse --git-path') echo .git/info/exclude ;;\n  'rev-parse --git-common-dir') echo .git ;;\n  *) exit 1 ;;\nesac\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(git, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let editor = home.path().join("bin/editor");
        std::fs::write(&editor, "#!/bin/sh\nexit 0\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(editor, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        Self { home, repo }
    }
    pub fn run(&self, args: &[&str]) -> Output {
        self.run_with_env(args, &[])
    }
    pub fn run_with_env(&self, args: &[&str], env: &[(&str, &str)]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_tkm"))
            .args(args)
            .env_clear()
            .env("HOME", self.home.path())
            .env("PATH", self.home.path().join("bin"))
            .env("EDITOR", self.home.path().join("bin/editor"))
            .env("XDG_DATA_HOME", self.home.path().join("data"))
            .env("XDG_CONFIG_HOME", self.home.path().join("config"))
            .env("XDG_CACHE_HOME", self.home.path().join("cache"))
            .env("XDG_RUNTIME_DIR", self.home.path().join("run"))
            .envs(env.iter().copied())
            .current_dir(&self.repo)
            .output()
            .unwrap()
    }
    pub fn seed_db(&self) {
        let paths = tkm_config::paths::Paths::from_dirs(
            self.home.path(),
            Some(&self.home.path().join("data")),
            Some(&self.home.path().join("config")),
            Some(&self.home.path().join("run")),
        );
        tkm_store::Store::open(paths.db).unwrap();
    }
}
