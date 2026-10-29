//! Canonical patch changes shared by live hooks and completed rollout items.

use std::path::{Path, PathBuf};

use serde_json::{Map, Value, json};
use tkm_core::model::{EditKind, EditVia, FileEdit};

use super::repo_relative;

pub(super) struct Change {
    op: &'static str,
    path: PathBuf,
    content: String,
}

impl Change {
    pub(super) fn edit(&self) -> FileEdit {
        FileEdit {
            path: self.path.clone(),
            kind: match self.op {
                "add" => EditKind::Create,
                "delete" => EditKind::Delete,
                _ => EditKind::Modify,
            },
            via: EditVia::Tool,
        }
    }
}

/// Ignore hunk positions and context: live patch syntax and generated unified
/// diffs describe those differently. Preserve every added and removed line.
fn update_content(text: &str) -> String {
    let mut in_hunk = false;
    let mut content = String::new();
    for line in text.lines() {
        if line.starts_with("@@") {
            in_hunk = true;
        } else if in_hunk && line.starts_with(['+', '-']) {
            content.push_str(line);
            content.push('\n');
        }
    }
    content
}

fn change(
    op: &'static str,
    path: &str,
    content: &str,
    move_to: Option<&str>,
    cwd: Option<&Path>,
) -> Option<Change> {
    let path = repo_relative(path, cwd)?;
    let mut content = match op {
        "add" => content.to_owned(),
        "update" => update_content(content),
        // The live delete syntax supplies no previous content.
        _ => String::new(),
    };
    if let Some(destination) = move_to.and_then(|path| repo_relative(path, cwd)) {
        content = format!("*** Move to: {}\n{content}", destination.display());
    }
    Some(Change { op, path, content })
}

pub(super) fn from_text(text: &str, cwd: Option<&Path>) -> Vec<Change> {
    let markers = [
        ("*** Add File: ", "add"),
        ("*** Update File: ", "update"),
        ("*** Delete File: ", "delete"),
    ];
    let mut sections: Vec<(&'static str, &str, Vec<&str>)> = Vec::new();
    for line in text.lines() {
        if let Some((op, path)) = markers
            .iter()
            .find_map(|(marker, op)| line.strip_prefix(*marker).map(|path| (*op, path)))
        {
            sections.push((op, path, Vec::new()));
        } else if let Some((_, _, lines)) = sections.last_mut() {
            lines.push(line);
        }
    }
    sections
        .into_iter()
        .filter_map(|(op, path, lines)| {
            let move_to = lines
                .iter()
                .find_map(|line| line.strip_prefix("*** Move to: "));
            let content = if op == "add" {
                lines.iter().filter_map(|line| line.strip_prefix('+')).fold(
                    String::new(),
                    |mut content, line| {
                        content.push_str(line);
                        content.push('\n');
                        content
                    },
                )
            } else {
                lines.join("\n")
            };
            change(op, path, &content, move_to, cwd)
        })
        .collect()
}

pub(super) fn from_items(items: &Map<String, Value>, cwd: Option<&Path>) -> Vec<Change> {
    items
        .iter()
        .filter_map(|(path, item)| {
            let op = match item.get("type").and_then(Value::as_str)? {
                "add" => "add",
                "update" => "update",
                "delete" => "delete",
                _ => return None,
            };
            let content = item
                .get(if op == "update" {
                    "unified_diff"
                } else {
                    "content"
                })
                .and_then(Value::as_str)
                .unwrap_or_default();
            let move_to = item.get("move_path").and_then(Value::as_str);
            change(op, path, content, move_to, cwd)
        })
        .collect()
}

pub(super) fn args_digest(changes: &[Change]) -> String {
    let mut entries: Vec<_> = changes
        .iter()
        .map(|change| (change.op, change.path.to_string_lossy(), &change.content))
        .collect();
    entries.sort_unstable();
    crate::digest::args_digest("apply_patch", &json!(entries))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn canonical_patch_changes_match_add_update_move_and_delete() {
        let cwd = Some(Path::new("/repo"));
        let text = "*** Begin Patch\n*** Delete File: old.txt\n*** Update File: src/a.rs\n*** Move to: src/b.rs\n@@ function\n-old\n+new\n*** Add File: new.txt\n+first\n+\n+third\n*** End Patch\n";
        let items = json!({
            "/repo/new.txt":{"type":"add","content":"first\n\nthird\n"},
            "/repo/src/a.rs":{"type":"update","unified_diff":"--- a/src/a.rs\n+++ b/src/b.rs\n@@ -1 +1 @@\n-old\n+new\n","move_path":"/repo/src/b.rs"},
            "/repo/old.txt":{"type":"delete","content":"previous content unavailable to live hook"}
        });
        let live = from_text(text, cwd);
        let reader = from_items(items.as_object().unwrap(), cwd);
        assert_eq!(args_digest(&live), args_digest(&reader));
        assert!(args_digest(&live).contains("first\\n\\nthird\\n"));
    }

    #[test]
    fn update_digest_ignores_hunk_syntax_between_live_and_rollout() {
        let cwd = Some(Path::new("/repo"));
        let live = from_text(
            "*** Begin Patch\n*** Update File: a.txt\n-old\n+new\n*** End Patch\n",
            cwd,
        );
        let items = json!({
            "/repo/a.txt":{"type":"update","unified_diff":"--- a/a.txt\n+++ b/a.txt\n@@ -1 +1 @@\n-old\n+new\n"}
        });
        let rollout = from_items(items.as_object().unwrap(), cwd);
        assert_eq!(args_digest(&live), args_digest(&rollout));
    }

    #[test]
    fn update_digest_ignores_interleaved_versus_grouped_hunks() {
        let cwd = Some(Path::new("/repo"));
        let live = from_text(
            "*** Update File: a.txt\n@@\n-one\n+uno\n-two\n+dos\n*** End Patch\n",
            cwd,
        );
        let items = json!({
            "/repo/a.txt":{"type":"update","unified_diff":"--- a/a.txt\n+++ b/a.txt\n@@ -1,2 +1,2 @@\n-one\n-two\n+uno\n+dos\n"}
        });
        let rollout = from_items(items.as_object().unwrap(), cwd);
        assert_eq!(args_digest(&live), args_digest(&rollout));
    }

    #[test]
    fn canonical_patch_digest_retains_changed_content() {
        let first = from_text("*** Add File: a.txt\n+one\n*** End Patch\n", None);
        let second = from_text("*** Add File: a.txt\n+two\n*** End Patch\n", None);
        assert_ne!(args_digest(&first), args_digest(&second));
    }
}
