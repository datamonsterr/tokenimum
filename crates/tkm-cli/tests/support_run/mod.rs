use std::process::{Command, Output};
use tempfile::TempDir;

pub struct Harness {
    home: TempDir,
}

impl Harness {
    pub fn new() -> Self {
        let home = tempfile::tempdir().unwrap();
        for dir in ["bin", "data", "config", "run", "cache"] {
            std::fs::create_dir(home.path().join(dir)).unwrap();
        }
        Self { home }
    }

    #[cfg(unix)]
    pub fn script(&self, name: &str, body: &str) {
        use std::os::unix::fs::PermissionsExt;
        let path = self.home.path().join("bin").join(name);
        std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    pub fn run(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_tkm"))
            .args(args)
            .env_clear()
            .env("HOME", self.home.path())
            .env("PATH", self.home.path().join("bin"))
            .env("XDG_DATA_HOME", self.home.path().join("data"))
            .env("XDG_CONFIG_HOME", self.home.path().join("config"))
            .env("XDG_RUNTIME_DIR", self.home.path().join("run"))
            .env("XDG_CACHE_HOME", self.home.path().join("cache"))
            .env("GIT_CEILING_DIRECTORIES", self.home.path())
            .current_dir(self.home.path())
            .output()
            .unwrap()
    }
}
