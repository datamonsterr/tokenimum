use std::process::{Command, Output};

pub fn tkm(args: &[&str]) -> Output {
    let home = tempfile::tempdir().unwrap();
    for directory in ["bin", "data", "config", "run", "cache"] {
        std::fs::create_dir(home.path().join(directory)).unwrap();
    }
    Command::new(env!("CARGO_BIN_EXE_tkm"))
        .args(args)
        .env_clear()
        .env("HOME", home.path())
        .env("PATH", home.path().join("bin"))
        .env("XDG_DATA_HOME", home.path().join("data"))
        .env("XDG_CONFIG_HOME", home.path().join("config"))
        .env("XDG_CACHE_HOME", home.path().join("cache"))
        .env("XDG_RUNTIME_DIR", home.path().join("run"))
        .env(
            "TKM_CONFIG",
            home.path().join("config/tokenimum/config.toml"),
        )
        .env("GIT_CEILING_DIRECTORIES", home.path())
        .current_dir(home.path())
        .output()
        .unwrap()
}
