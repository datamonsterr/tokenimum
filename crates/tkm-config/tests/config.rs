#![forbid(unsafe_code)]
#![allow(clippy::unwrap_used, clippy::expect_used)]
use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};
use tkm_config::config::{
    ConfigError, ConfigFile, Layer, LoadInputs, diff_from_preset, explain, is_disabled, load,
};
static NEXT: AtomicU64 = AtomicU64::new(0);
struct Temp(PathBuf);
impl Temp {
    fn new() -> Self {
        let p = std::env::temp_dir().join(format!(
            "tkm-config-{}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&p).unwrap();
        Self(p)
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn inputs(t: &Temp) -> LoadInputs {
    LoadInputs {
        global_path: t.0.join("global.toml"),
        repo_root: Some(t.0.clone()),
        ..LoadInputs::default()
    }
}
#[test]
fn layer_precedence_all_seven_layers() {
    let t = Temp::new();
    let mut i = inputs(&t);
    fs::write(&i.global_path, "[inject]\nmax_tokens=101\n").unwrap();
    fs::write(t.0.join(".tokenimum.toml"), "[inject]\nmax_tokens=102\n").unwrap();
    fs::write(
        t.0.join(".tokenimum.local.toml"),
        "[inject]\nmax_tokens=103\n",
    )
    .unwrap();
    i.env.push(("TKM_INJECT_MAX_TOKENS".into(), "104".into()));
    i.flags.push("inject.max_tokens=105".into());
    for (v, layer) in [
        (105, Layer::Flags),
        (104, Layer::Env),
        (103, Layer::RepoLocal),
        (102, Layer::Repo),
        (101, Layer::Global),
        (900, Layer::Preset),
        (600, Layer::Default),
    ] {
        let c = load(&i).unwrap();
        assert_eq!(c.get_int("inject.max_tokens"), Some(v));
        assert_eq!(c.source("inject.max_tokens"), Some(layer));
        match layer {
            Layer::Flags => i.flags.clear(),
            Layer::Env => i.env.clear(),
            Layer::RepoLocal => fs::remove_file(t.0.join(".tokenimum.local.toml")).unwrap(),
            Layer::Repo => fs::remove_file(t.0.join(".tokenimum.toml")).unwrap(),
            Layer::Global => fs::write(&i.global_path, "preset=\"aggressive\"\n").unwrap(),
            Layer::Preset => fs::remove_file(&i.global_path).unwrap(),
            Layer::Default => {}
        }
    }
}
#[test]
fn restricted_repo_entries_are_ignored_in_both_layers() {
    for (key, val) in [
        ("decide.budget_daily_usd", "1.0"),
        ("decide.budget_session_usd", "1.0"),
        ("decide.api_key_path", "\"x\""),
        ("prune.mode", "\"apply\""),
        ("loop.escalate_command", "\"x\""),
    ] {
        for name in [".tokenimum.toml", ".tokenimum.local.toml"] {
            let t = Temp::new();
            let i = inputs(&t);
            fs::write(t.0.join(name), format!("{key} = {val}\n")).unwrap();
            let c = load(&i).unwrap();
            assert!(c.ignored.iter().any(|x| x.key == key));
            assert!(!matches!(
                c.source(key),
                Some(Layer::Repo | Layer::RepoLocal)
            ));
        }
    }
}
#[test]
fn repo_preset_filters_apply_and_allows_report() {
    let t = Temp::new();
    let i = inputs(&t);
    fs::write(t.0.join(".tokenimum.toml"), "preset=\"aggressive\"\n").unwrap();
    let c = load(&i).unwrap();
    assert_ne!(c.get_str("prune.mode"), Some("apply"));
    assert!(c.ignored.iter().any(|x| x.key == "prune.mode"));
    fs::write(t.0.join(".tokenimum.toml"), "[prune]\nmode=\"report\"\n").unwrap();
    assert_eq!(load(&i).unwrap().get_str("prune.mode"), Some("report"));
}
#[test]
fn preset_tables_match_spec_10_3() {
    for (preset, key, value) in [
        ("observe", "loop.mode", "shadow"),
        ("observe", "prune.mode", "report"),
        ("balanced", "loop.mode", "redirect"),
        ("balanced", "prune.mode", "propose"),
        ("aggressive", "loop.level", "aggressive"),
        ("aggressive", "prune.mode", "apply"),
        ("offline", "facts.evidence", "code"),
    ] {
        let t = Temp::new();
        let mut i = inputs(&t);
        i.flags.push(format!("preset={preset}"));
        assert_eq!(load(&i).unwrap().get_str(key), Some(value));
    }
}
#[test]
fn env_mapping_redirect_disable_and_validation() {
    let t = Temp::new();
    let mut i = inputs(&t);
    let alternate = t.0.join("other.toml");
    fs::write(&alternate, "[loop]\nmode=\"shadow\"\n").unwrap();
    i.env = vec![
        (
            "TKM_CONFIG".into(),
            alternate.to_string_lossy().into_owned(),
        ),
        ("TKM_HOSTS_CLAUDE_ENABLED".into(), "false".into()),
        ("TKM_DISABLE".into(), "true".into()),
        ("TKM_UNKNOWN".into(), "x".into()),
    ];
    let c = load(&i).unwrap();
    assert_eq!(c.get_str("loop.mode"), Some("shadow"));
    assert_eq!(c.get_bool("hosts.claude.enabled"), Some(false));
    assert!(is_disabled(&i.env));
    assert!(!c.errors.is_empty());
    i.flags = vec![
        "loop.mode=wrong".into(),
        "facts.fire_gate=2.0".into(),
        "facts.enabled=oops".into(),
        "missing.key=1".into(),
    ];
    assert!(load(&i).unwrap().errors.len() >= 4);
}
#[test]
fn edit_preserves_comments_explains_and_diffs() {
    let t = Temp::new();
    let i = inputs(&t);
    fs::write(
        &i.global_path,
        "# keep\n[loop]\nmode = \"shadow\"\nlevel = \"default\"\n",
    )
    .unwrap();
    let mut f = ConfigFile::load(i.global_path.clone(), Layer::Global).unwrap();
    f.set("loop.mode", "redirect").unwrap();
    f.unset("loop.level").unwrap();
    f.save().unwrap();
    let text = fs::read_to_string(&i.global_path).unwrap();
    assert!(text.contains("# keep"));
    assert!(!text.contains("level"));
    let c = load(&i).unwrap();
    assert!(explain(&c, "loop.mode").unwrap().contains("redirect"));
    assert!(diff_from_preset(&c).is_empty());
    let mut repo = ConfigFile::load(t.0.join(".tokenimum.toml"), Layer::Repo).unwrap();
    assert!(
        matches!(repo.set("prune.mode", "apply"), Err(ConfigError::RestrictedInRepo { key }) if key == "prune.mode")
    );
}

fn restricted_key_both_repo_layers(key: &str, raw: &str) {
    for name in [".tokenimum.toml", ".tokenimum.local.toml"] {
        let t = Temp::new();
        let i = inputs(&t);
        fs::write(t.0.join(name), format!("{key} = {raw}\n")).unwrap();
        let c = load(&i).unwrap();
        let layer = if name == ".tokenimum.toml" {
            Layer::Repo
        } else {
            Layer::RepoLocal
        };
        let mut file = ConfigFile::load(t.0.join(name), layer).unwrap();
        assert!(
            matches!(file.set(key, raw), Err(ConfigError::RestrictedInRepo { key: rejected }) if rejected == key)
        );
        assert!(c.ignored.iter().any(|entry| entry.key == key));
        assert!(!matches!(
            c.source(key),
            Some(Layer::Repo | Layer::RepoLocal)
        ));
    }
}
#[test]
fn repo_cannot_set_daily_budget() {
    restricted_key_both_repo_layers("decide.budget_daily_usd", "1.0");
}
#[test]
fn repo_cannot_set_session_budget() {
    restricted_key_both_repo_layers("decide.budget_session_usd", "1.0");
}
#[test]
fn repo_cannot_set_api_key_path() {
    restricted_key_both_repo_layers("decide.api_key_path", "\"other\"");
}
#[test]
fn repo_cannot_enable_prune_apply() {
    restricted_key_both_repo_layers("prune.mode", "\"apply\"");
}
#[test]
fn repo_cannot_set_escalation_command() {
    restricted_key_both_repo_layers("loop.escalate_command", "\"other\"");
}
#[test]
fn trusted_layers_can_set_restricted_values() {
    for (key, toml, text) in [
        ("decide.budget_daily_usd", "1.0", "1.0"),
        ("decide.budget_session_usd", "1.0", "1.0"),
        ("decide.api_key_path", "\"other\"", "other"),
        ("prune.mode", "\"apply\"", "apply"),
        ("loop.escalate_command", "\"other\"", "other"),
    ] {
        for layer in [Layer::Global, Layer::Env, Layer::Flags] {
            let t = Temp::new();
            let mut i = inputs(&t);
            match layer {
                Layer::Global => fs::write(&i.global_path, format!("{key} = {toml}\n")).unwrap(),
                Layer::Env => i.env.push((
                    format!("TKM_{}", key.replace('.', "_").to_uppercase()),
                    text.into(),
                )),
                Layer::Flags => i.flags.push(format!("{key}={text}")),
                _ => unreachable!(),
            }
            let c = load(&i).unwrap();
            assert_eq!(c.source(key), Some(layer));
            assert!(c.errors.is_empty());
        }
    }
}
#[test]
fn malformed_global_file_names_its_path() {
    let t = Temp::new();
    let i = inputs(&t);
    fs::write(&i.global_path, "[broken\n").unwrap();
    assert!(matches!(load(&i), Err(ConfigError::Parse { path, .. }) if path == i.global_path));
}
#[test]
fn preset_overrides_include_each_specified_value() {
    for (name, bools, ints) in [
        (
            "observe",
            vec![("inject.enabled", false), ("decide.shadow", true)],
            vec![],
        ),
        (
            "balanced",
            vec![("inject.enabled", true), ("decide.agreement", false)],
            vec![("inject.max_tokens", 600)],
        ),
        (
            "aggressive",
            vec![("decide.agreement", false)],
            vec![
                ("facts.promote_confirmations", 2),
                ("inject.max_tokens", 900),
            ],
        ),
        (
            "offline",
            vec![("decide.enabled", false), ("inject.memory", false)],
            vec![],
        ),
    ] {
        let t = Temp::new();
        let mut i = inputs(&t);
        i.flags.push(format!("preset={name}"));
        let c = load(&i).unwrap();
        for (key, expected) in bools {
            assert_eq!(c.get_bool(key), Some(expected), "{name}: {key}");
        }
        for (key, expected) in ints {
            assert_eq!(c.get_int(key), Some(expected), "{name}: {key}");
        }
    }
    let t = Temp::new();
    let mut i = inputs(&t);
    i.flags.push("preset=offline".into());
    assert_eq!(load(&i).unwrap().get_str("prune.mode"), Some("propose"));
}

#[test]
fn float_keys_accept_integers() {
    let t = Temp::new();
    let mut i = inputs(&t);
    fs::write(
        &i.global_path,
        "[facts]\nfire_gate=1\n[decide]\nbudget_daily_usd=1\n",
    )
    .unwrap();
    let c = load(&i).unwrap();
    assert_eq!(c.get_float("facts.fire_gate"), Some(1.0));
    assert_eq!(c.get_float("decide.budget_daily_usd"), Some(1.0));
    assert!(c.errors.is_empty());
    i.env.push(("TKM_FACTS_FIRE_GATE".into(), "0".into()));
    i.flags.push("decide.budget_daily_usd=2".into());
    let c = load(&i).unwrap();
    assert_eq!(c.get_float("facts.fire_gate"), Some(0.0));
    assert_eq!(c.get_float("decide.budget_daily_usd"), Some(2.0));
    let mut f = ConfigFile::load(i.global_path.clone(), Layer::Global).unwrap();
    f.set("facts.fire_gate", "1").unwrap();
    f.save().unwrap();
    assert_eq!(
        load(&inputs(&t)).unwrap().get_float("facts.fire_gate"),
        Some(1.0)
    );
}
#[test]
fn errors_identify_unknown_invalid_restricted_and_io() {
    let t = Temp::new();
    let mut i = inputs(&t);
    i.flags = vec!["bad.key=1".into(), "facts.fire_gate=2".into()];
    let c = load(&i).unwrap();
    assert!(
        c.errors
            .iter()
            .any(|e| matches!(e, ConfigError::UnknownKey(key) if key == "bad.key"))
    );
    assert!(
        c.errors.iter().any(
            |e| matches!(e, ConfigError::InvalidValue { key, .. } if key == "facts.fire_gate")
        )
    );
    for layer in [Layer::Repo, Layer::RepoLocal] {
        let mut f = ConfigFile::load(t.0.join("file"), layer).unwrap();
        assert!(
            matches!(f.set("decide.api_key_path", "other"), Err(ConfigError::RestrictedInRepo { key }) if key == "decide.api_key_path")
        );
        assert!(
            matches!(f.unset("bad.key"), Err(ConfigError::UnknownKey(key)) if key == "bad.key")
        );
    }
    let mut i = inputs(&t);
    i.global_path = t.0.clone();
    assert!(matches!(load(&i), Err(ConfigError::Io { path, .. }) if path == t.0));
}
