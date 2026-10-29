use std::fs;
use std::path::Path;

use toml_edit::{ArrayOfTables, DocumentMut, Item, Table};

use crate::{AdapterError, InstallContext, InstallReport};

const HOOK_SPECS: [(&str, tkm_proto::EventKind, u64); 6] = [
    ("session_start", tkm_proto::EventKind::SessionStart, 1),
    ("message_submit", tkm_proto::EventKind::Prompt, 3),
    ("tool_call_before", tkm_proto::EventKind::ToolBefore, 1),
    ("tool_call_after", tkm_proto::EventKind::ToolDone, 1),
    ("turn_end", tkm_proto::EventKind::TurnEnd, 1),
    ("session_end", tkm_proto::EventKind::SessionEnd, 1),
];

fn is_owned(table: &Table) -> bool {
    let Some(cmd) = table.get("command").and_then(Item::as_str) else {
        return false;
    };
    crate::writer::owns_handler(&serde_json::json!({ "command": cmd }), "deepseek")
}

fn already_matches(doc: &DocumentMut) -> bool {
    let Some(hooks_table) = doc.get("hooks").and_then(Item::as_table) else {
        return false;
    };
    let Some(hooks_arr) = hooks_table.get("hooks").and_then(Item::as_array_of_tables) else {
        return false;
    };
    let owned: Vec<&Table> = hooks_arr.iter().filter(|t| is_owned(t)).collect();
    if owned.len() != HOOK_SPECS.len() {
        return false;
    }
    for (table, &(ev, kind, timeout)) in owned.iter().zip(HOOK_SPECS.iter()) {
        let expected_cmd = format!("tkm-hook --host deepseek --event {}", kind.as_str());
        let expected_name = format!("tokenimum-{ev}");
        if table.get("event").and_then(Item::as_str) != Some(ev) {
            return false;
        }
        if table.get("command").and_then(Item::as_str) != Some(&expected_cmd) {
            return false;
        }
        if table.get("name").and_then(Item::as_str) != Some(&expected_name) {
            return false;
        }
        if table.get("timeout_secs").and_then(Item::as_integer)
            != i64::try_from(timeout).ok()
        {
            return false;
        }
        if table.get("continue_on_error").and_then(Item::as_bool) != Some(true) {
            return false;
        }
    }
    true
}

pub(super) fn install(context: &InstallContext) -> Result<InstallReport, AdapterError> {
    if context.repo_root.is_some() {
        return Err(AdapterError::Unsupported {
            operation: "codewhale project hooks need /hooks approve; use user scope".to_owned(),
        });
    }

    let root = &context.agent_home;
    let path = context.agent_home.join("config.toml");

    let edit = |original: Option<&str>| -> Result<String, AdapterError> {
        if let Some(orig_text) = original {
            if let Ok(doc) = orig_text.parse::<DocumentMut>() {
                if already_matches(&doc) {
                    return Ok(orig_text.to_owned());
                }
            }
        }

        let mut doc: DocumentMut = match original {
            Some(text) => text.parse().map_err(|e| AdapterError::InvalidPayload {
                message: format!("malformed config TOML: {e}"),
            })?,
            None => DocumentMut::new(),
        };

        if !doc.contains_key("hooks") {
            doc["hooks"] = Item::Table(Table::new());
        }
        let hooks_item = &mut doc["hooks"];
        let Some(hooks_table) = hooks_item.as_table_mut() else {
            return Err(AdapterError::InvalidPayload {
                message: "[hooks] is not a table in config.toml".to_owned(),
            });
        };

        if !hooks_table.contains_key("hooks") {
            hooks_table["hooks"] = Item::ArrayOfTables(ArrayOfTables::new());
        }
        let hooks_arr_item = &mut hooks_table["hooks"];
        let Some(hooks_arr) = hooks_arr_item.as_array_of_tables_mut() else {
            return Err(AdapterError::InvalidPayload {
                message: "hooks.hooks is not an array of tables in config.toml".to_owned(),
            });
        };

        hooks_arr.retain(|t| !is_owned(t));

        for (ev, kind, timeout) in HOOK_SPECS {
            let mut table = Table::new();
            table["event"] = toml_edit::value(ev);
            table["command"] = toml_edit::value(format!(
                "tkm-hook --host deepseek --event {}",
                kind.as_str()
            ));
            table["name"] = toml_edit::value(format!("tokenimum-{ev}"));
            table["timeout_secs"] =
                toml_edit::value(i64::try_from(timeout).unwrap_or(1));
            table["continue_on_error"] = toml_edit::value(true);
            hooks_arr.push(table);
        }

        Ok(doc.to_string())
    };

    let mut report =
        crate::writer::install_text_file(root, &path, context.dry_run, &edit)?;

    let config_text = fs::read_to_string(&path).ok();
    let doc_opt: Option<DocumentMut> = config_text.as_deref().and_then(|s| s.parse().ok());
    if let Some(doc) = doc_opt {
        if let Some(hooks_table) = doc.get("hooks").and_then(Item::as_table) {
            if hooks_table.get("enabled").and_then(Item::as_bool) == Some(false) {
                report.instructions.push(
                    "Codewhale hooks are disabled in config ([hooks] enabled = false); enable hooks for tokenimum to receive events.".to_owned(),
                );
            }
        }
    }
    report.instructions.push(
        "Note that [hooks] default_timeout_secs, when set, overrides tokenimum timeouts.".to_owned(),
    );

    Ok(report)
}

pub(super) fn uninstall(context: &InstallContext) -> Result<InstallReport, AdapterError> {
    if context.repo_root.is_some() {
        return Err(AdapterError::Unsupported {
            operation: "codewhale project hooks need /hooks approve; use user scope".to_owned(),
        });
    }

    let root = &context.agent_home;
    let path = context.agent_home.join("config.toml");

    let edit = |text: &str| -> Result<String, AdapterError> {
        let mut doc: DocumentMut = text.parse().map_err(|e| AdapterError::InvalidPayload {
            message: format!("malformed config TOML: {e}"),
        })?;
        if let Some(hooks_table) = doc.get_mut("hooks").and_then(Item::as_table_mut) {
            if let Some(hooks_arr) =
                hooks_table.get_mut("hooks").and_then(Item::as_array_of_tables_mut)
            {
                hooks_arr.retain(|t| !is_owned(t));
                if hooks_arr.is_empty() {
                    hooks_table.remove("hooks");
                }
            }
            if hooks_table.is_empty() {
                doc.remove("hooks");
            }
        }
        Ok(doc.to_string())
    };

    let same = |a: &str, b: &str| -> Result<bool, AdapterError> {
        if a == b {
            return Ok(true);
        }
        let val_a: Result<serde_json::Value, _> = toml_edit::de::from_str(a);
        let val_b: Result<serde_json::Value, _> = toml_edit::de::from_str(b);
        match (val_a, val_b) {
            (Ok(va), Ok(vb)) => Ok(va == vb),
            _ => Ok(false),
        }
    };

    let empty = |text: &str| -> Result<bool, AdapterError> {
        if text.trim().is_empty() {
            return Ok(true);
        }
        let doc: DocumentMut = match text.parse() {
            Ok(d) => d,
            Err(_) => return Ok(false),
        };
        Ok(doc.as_table().is_empty())
    };

    crate::writer::uninstall_text_file(
        root,
        &path,
        context.dry_run,
        &edit,
        &same,
        &empty,
    )
}
