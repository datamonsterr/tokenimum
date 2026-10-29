//! Kimi Code hook installation and removal in `config.toml`.

use toml_edit::{ArrayOfTables, DocumentMut, Item, Table};

use crate::{AdapterError, InstallContext, InstallReport, writer};

struct DesiredHook {
    event: &'static str,
    command: &'static str,
    timeout: i64,
}

const DESIRED_HOOKS: [DesiredHook; 6] = [
    DesiredHook {
        event: "SessionStart",
        command: "tkm-hook --host kimi --event session_start",
        timeout: 1,
    },
    DesiredHook {
        event: "UserPromptSubmit",
        command: "tkm-hook --host kimi --event prompt",
        timeout: 3,
    },
    DesiredHook {
        event: "PostToolUse",
        command: "tkm-hook --host kimi --event tool_done",
        timeout: 1,
    },
    DesiredHook {
        event: "PostToolUseFailure",
        command: "tkm-hook --host kimi --event tool_done",
        timeout: 1,
    },
    DesiredHook {
        event: "Stop",
        command: "tkm-hook --host kimi --event turn_end",
        timeout: 1,
    },
    DesiredHook {
        event: "SessionEnd",
        command: "tkm-hook --host kimi --event session_end",
        timeout: 5,
    },
];

fn is_owned(table: &Table) -> bool {
    let Some(cmd) = table.get("command").and_then(Item::as_str) else {
        return false;
    };
    writer::owns_handler(&serde_json::json!({ "command": cmd }), "kimi")
}

fn already_matches(doc: &DocumentMut) -> bool {
    let Some(hooks_arr) = doc.get("hooks").and_then(Item::as_array_of_tables) else {
        return false;
    };
    let owned: Vec<&Table> = hooks_arr.iter().filter(|t| is_owned(t)).collect();
    if owned.len() != DESIRED_HOOKS.len() {
        return false;
    }
    for (table, desired) in owned.iter().zip(DESIRED_HOOKS.iter()) {
        if table.get("event").and_then(Item::as_str) != Some(desired.event) {
            return false;
        }
        if table.get("command").and_then(Item::as_str) != Some(desired.command) {
            return false;
        }
        if table.get("timeout").and_then(Item::as_integer) != Some(desired.timeout) {
            return false;
        }
    }
    true
}

pub(super) fn install(context: &InstallContext) -> Result<InstallReport, AdapterError> {
    if context.repo_root.is_some() {
        return Err(AdapterError::Unsupported {
            operation: "kimi project-scope hooks".into(),
        });
    }

    let root = &context.agent_home;
    let path = context.agent_home.join("config.toml");

    let edit = |original: Option<&str>| -> Result<String, AdapterError> {
        if let Some(orig_text) = original
            && let Ok(doc) = orig_text.parse::<DocumentMut>()
            && already_matches(&doc)
        {
            return Ok(orig_text.to_owned());
        }

        let mut doc: DocumentMut = match original {
            Some(text) => text.parse().map_err(|e| AdapterError::InvalidPayload {
                message: format!("malformed config TOML: {e}"),
            })?,
            None => DocumentMut::new(),
        };

        if let Some(item) = doc.get("hooks")
            && !item.is_array_of_tables()
        {
            return Err(AdapterError::InvalidPayload {
                message: "hooks must be an array of tables".into(),
            });
        }

        if !doc.contains_key("hooks") {
            doc["hooks"] = Item::ArrayOfTables(ArrayOfTables::new());
        }

        let hooks_arr = doc
            .get_mut("hooks")
            .and_then(Item::as_array_of_tables_mut)
            .ok_or_else(|| AdapterError::InvalidPayload {
                message: "hooks must be an array of tables".into(),
            })?;

        let mut carry = crate::toml_util::remove_owned(hooks_arr, is_owned);

        for desired in &DESIRED_HOOKS {
            let mut table = Table::new();
            let kept = std::mem::take(&mut carry);
            table.decor_mut().set_prefix(format!("{kept}# tokenimum\n"));
            table["event"] = toml_edit::value(desired.event);
            table["command"] = toml_edit::value(desired.command);
            table["timeout"] = toml_edit::value(desired.timeout);
            hooks_arr.push(table);
        }

        Ok(doc.to_string())
    };

    writer::install_text_file(root, &path, context.dry_run, &edit)
}

pub(super) fn uninstall(context: &InstallContext) -> Result<InstallReport, AdapterError> {
    if context.repo_root.is_some() {
        return Err(AdapterError::Unsupported {
            operation: "kimi project-scope hooks".into(),
        });
    }

    let root = &context.agent_home;
    let path = context.agent_home.join("config.toml");

    let edit = |text: &str| -> Result<String, AdapterError> {
        if text.trim().is_empty() {
            return Ok(text.to_owned());
        }
        let mut doc: DocumentMut = text.parse().map_err(|e| AdapterError::InvalidPayload {
            message: format!("malformed config TOML: {e}"),
        })?;

        let mut leftover = String::new();
        if let Some(hooks_item) = doc.get_mut("hooks") {
            let Some(hooks_arr) = hooks_item.as_array_of_tables_mut() else {
                return Err(AdapterError::InvalidPayload {
                    message: "hooks must be an array of tables".into(),
                });
            };
            let had_foreign = hooks_arr.iter().any(|t| !is_owned(t));
            leftover = crate::toml_util::remove_owned(hooks_arr, is_owned);
            if hooks_arr.is_empty() && !had_foreign {
                doc.remove("hooks");
            }
        }
        if !leftover.is_empty() {
            let existing = doc.trailing().as_str().unwrap_or_default().to_owned();
            doc.set_trailing(format!("{existing}{leftover}"));
        }

        Ok(doc.to_string())
    };

    let same = |a: &str, b: &str| -> Result<bool, AdapterError> { Ok(a == b) };

    let empty = |text: &str| -> Result<bool, AdapterError> { Ok(text.trim().is_empty()) };

    writer::uninstall_text_file(root, &path, context.dry_run, &edit, &same, &empty)
}
