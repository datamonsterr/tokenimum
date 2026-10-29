use super::{BenchError, types::invalid};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

/// Refuse any configuration drift except exact Claude hooks and files written by `tkm init`.
///
/// Inputs are snapshots containing both home and project AI configuration. Runtime logs and
/// databases are not configuration and must not be placed in these snapshots.
///
/// # Errors
/// Rejects changed foreign settings, skills, plugins, MCP servers, symlinks and spoofed hooks.
pub fn check_config(a: &Path, b: &Path) -> Result<(), BenchError> {
    let a = files(a)?;
    let b = files(b)?;
    let paths: BTreeSet<_> = a.keys().chain(b.keys()).collect();
    for path in paths {
        let before = a.get(path);
        let after = b.get(path);
        if before == after {
            continue;
        }
        let name = path.to_string_lossy();
        if name == ".claude/settings.json" || name == "project/.claude/settings.local.json" {
            let left = before.map_or(Ok(json!({})), |bytes| serde_json::from_slice(bytes))?;
            let mut right: Value =
                after.map_or(Ok(json!({})), |bytes| serde_json::from_slice(bytes))?;
            strip_init_hooks(&mut right);
            let mut left: Value = left;
            normalize_empty_hooks(&mut left);
            normalize_empty_hooks(&mut right);
            if left == right {
                continue;
            }
        }
        if name == ".claude/skills/tokenimum/SKILL.md"
            && before.is_none()
            && after.is_some_and(|bytes| {
                bytes.as_slice()
                    == tkm_hosts::fold_only::skill::render(tkm_proto::Host::Claude).as_bytes()
            })
        {
            continue;
        }
        if let Some(original) = name.strip_suffix(".tkm-bak") {
            let original = Path::new(original);
            if before.is_none()
                && matches!(
                    original.to_str(),
                    Some(".claude/settings.json" | "project/.claude/settings.local.json")
                )
                && after.is_some_and(|bytes| {
                    a.get(original).map_or(bytes.is_empty(), |old| old == bytes)
                })
            {
                continue;
            }
        }
        if before.is_none()
            && after.is_some()
            && matches!(
                name.as_ref(),
                ".config/tokenimum/config.toml" | ".config/systemd/user/tkmd.service"
            )
        {
            continue;
        }
        return Err(invalid(
            "A/B configuration differs beyond tkm init files and exact hook entries",
        ));
    }
    Ok(())
}

fn strip_init_hooks(value: &mut Value) {
    let Some(events) = value.get_mut("hooks").and_then(Value::as_object_mut) else {
        return;
    };
    for (event, kind, timeout, matcher) in [
        ("SessionStart", "session_start", 1, false),
        ("UserPromptSubmit", "prompt", 3, false),
        ("PostToolUse", "tool_done", 1, true),
        ("PostToolUseFailure", "tool_done", 1, true),
        ("Stop", "turn_end", 1, false),
        ("SessionEnd", "session_end", 1, false),
    ] {
        let mut expected = json!({"hooks":[{"type":"command","command":"tkm-hook","args":["--host","claude","--event",kind],"timeout":timeout}]});
        if matcher {
            expected["matcher"] = json!("*");
        }
        if let Some(groups) = events.get_mut(event).and_then(Value::as_array_mut) {
            groups.retain(|group| group != &expected);
        }
    }
}
fn normalize_empty_hooks(value: &mut Value) {
    if let Some(events) = value.get_mut("hooks").and_then(Value::as_object_mut) {
        events.retain(|_, groups| !groups.as_array().is_some_and(Vec::is_empty));
    }
    if value
        .get("hooks")
        .and_then(Value::as_object)
        .is_some_and(serde_json::Map::is_empty)
        && let Some(root) = value.as_object_mut()
    {
        root.remove("hooks");
    }
}
fn files(root: &Path) -> Result<BTreeMap<PathBuf, Vec<u8>>, BenchError> {
    fn walk(
        root: &Path,
        dir: &Path,
        result: &mut BTreeMap<PathBuf, Vec<u8>>,
    ) -> Result<(), BenchError> {
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let metadata = entry.file_type()?;
            if metadata.is_symlink() {
                return Err(invalid("configuration snapshot contains symlink"));
            }
            let path = entry.path();
            if metadata.is_dir() {
                walk(root, &path, result)?;
            } else if metadata.is_file() {
                result.insert(
                    path.strip_prefix(root)
                        .map_err(|_| invalid("configuration path escapes snapshot"))?
                        .to_path_buf(),
                    fs::read(path)?,
                );
            } else {
                return Err(invalid("configuration snapshot contains special file"));
            }
        }
        Ok(())
    }
    let mut result = BTreeMap::new();
    walk(root, root, &mut result)?;
    Ok(result)
}
