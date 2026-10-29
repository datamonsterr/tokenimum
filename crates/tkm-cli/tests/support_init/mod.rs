#![allow(dead_code)] // Each integration target uses a different subset of this fixture.

use std::collections::BTreeMap;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

pub const HOMES: [(&str, &str, &str); 8] = [
    ("claude", ".claude", "settings.json"),
    ("codex", ".codex", "hooks.json"),
    ("agy", ".gemini", "config/hooks.json"),
    ("cursor", ".cursor", "hooks.json"),
    ("kimi", ".kimi", "config.toml"),
    ("deepseek", ".codewhale", "config.toml"),
    ("opencode", ".config/opencode", "opencode.json"),
    ("pi", ".pi/agent", "settings.json"),
];

pub struct Fixture {
    pub temp: tempfile::TempDir,
    pub home: PathBuf,
    pub bin: PathBuf,
    pub paths: tkm_config::paths::Paths,
}

impl Fixture {
    pub fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path().join("home");
        let bin = temp.path().join("bin");
        for path in [&home, &bin] {
            fs::create_dir(path).unwrap();
        }
        for name in ["config", "data", "state", "run", "cache"] {
            fs::create_dir(temp.path().join(name)).unwrap();
        }
        let paths = tkm_config::paths::Paths::from_dirs(
            &home,
            Some(&temp.path().join("data")),
            Some(&temp.path().join("config")),
            Some(&temp.path().join("run")),
        );
        let fixture = Self {
            temp,
            home,
            bin,
            paths,
        };
        for (host, relative, config) in HOMES {
            let path = fixture.home.join(relative).join(config);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            let contents = match host {
                "claude" | "codex" => {
                    "{\n  \"hooks\": {\"SessionStart\": [{\"hooks\": [{\"type\": \"command\", \"command\": \"foreign-hook\"}]}]},\n  \"foreign\": true\n}\n"
                }
                "cursor" => {
                    "{\"version\":1, \"hooks\":{\"sessionStart\":[{\"command\":\"foreign-hook\"}]},\"foreign\":true}\n"
                }
                "kimi" => {
                    "# foreign config\n[[hooks]]\nevent = \"SessionStart\"\ncommand = \"foreign-hook\"\n"
                }
                "deepseek" => {
                    "# foreign config\n[[hooks.hooks]]\nevent = \"session_start\"\ncommand = \"foreign-hook\"\n"
                }
                _ => "{\"foreign\": {\"command\": \"foreign-hook\"}}\n",
            };
            fs::write(path, contents).unwrap();
            fixture.script(host, "printf 'fixture 1.0\\n'\n");
        }
        for binary in ["systemctl", "launchctl", "tkmd"] {
            fixture.script(
                binary,
                &format!("printf '{binary} %s\\n' \"$*\" >> \"$CALL_LOG\"\n"),
            );
        }
        fixture
    }

    pub fn script(&self, name: &str, body: &str) {
        let path = self.bin.join(name);
        fs::write(&path, format!("#!/bin/sh\n{body}")).unwrap();
        fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
    }

    pub fn command(&self, args: &[&str]) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_tkm"));
        command
            .args(args)
            .env_clear()
            .env("HOME", &self.home)
            .env("PATH", &self.bin)
            .env("CALL_LOG", self.temp.path().join("calls.log"))
            .env("TKM_CONFIG", &self.paths.global_config)
            .env("GIT_CEILING_DIRECTORIES", self.temp.path())
            .current_dir(&self.home)
            .stdin(Stdio::null());
        for (name, directory) in [
            ("XDG_CONFIG_HOME", "config"),
            ("XDG_DATA_HOME", "data"),
            ("XDG_STATE_HOME", "state"),
            ("XDG_RUNTIME_DIR", "run"),
            ("XDG_CACHE_HOME", "cache"),
        ] {
            command.env(name, self.temp.path().join(directory));
        }
        command
    }

    pub fn run(&self, args: &[&str]) -> Output {
        self.command(args).output().unwrap()
    }

    pub fn init(&self) {
        let output = self.run(&["init", "--yes"]);
        assert!(output.status.success(), "{output:?}");
    }

    pub fn installs(&self) -> Vec<tkm_store::InstallRecord> {
        tkm_store::Store::open(&self.paths.db)
            .unwrap()
            .list_installs()
            .unwrap()
    }

    pub fn repository(&self) -> PathBuf {
        let repo = self.temp.path().join("repository");
        fs::create_dir_all(repo.join(".git/info")).unwrap();
        fs::write(repo.join(".git/HEAD"), "ref: refs/heads/main\n").unwrap();
        fs::write(
            repo.join(".git/config"),
            "[core]\nrepositoryformatversion = 0\n",
        )
        .unwrap();
        fs::write(repo.join(".git/info/exclude"), "# foreign exclude\n").unwrap();
        self.script("git", "case \"$1 $2\" in\n'ls-files --error-unmatch') exit 1;;\n'rev-parse --git-path') printf '.git/info/exclude\\n';;\n'rev-parse --git-common-dir') printf '.git\\n';;\n*) exit 2;;\nesac\n");
        repo
    }
}

pub type Snapshot = BTreeMap<PathBuf, (Vec<u8>, u32)>;

pub fn snapshot(root: &Path) -> Snapshot {
    fn visit(root: &Path, current: &Path, entries: &mut Snapshot) {
        let metadata = fs::symlink_metadata(current).unwrap();
        let bytes = if metadata.is_file() {
            fs::read(current).unwrap()
        } else {
            Vec::new()
        };
        entries.insert(
            current.strip_prefix(root).unwrap().to_path_buf(),
            (bytes, metadata.permissions().mode()),
        );
        if metadata.is_dir() {
            for entry in fs::read_dir(current).unwrap() {
                visit(root, &entry.unwrap().path(), entries);
            }
        }
    }
    let mut entries = BTreeMap::new();
    visit(root, root, &mut entries);
    entries
}

pub fn stdout(output: &Output) -> String {
    String::from_utf8(output.stdout.clone()).unwrap()
}
