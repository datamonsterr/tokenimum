use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::PathBuf;

use serde_json::Value;
use tkm_core::model::{Call, Host, Session, StopReason, Turn, Usage};

use crate::{AdapterError, SessionSource};

// Per-turn usage in Codewhale is not tracked in the session file, which contains
// only aggregate metadata.total_tokens. Per-turn token usage comes strictly from
// live turn_end hook events. The session and turn usages here remain zeroed.

pub(super) fn read_session(source: &SessionSource) -> Result<Session, AdapterError> {
    let file_path = if source.path.as_os_str().is_empty() {
        source
            .agent_home
            .join("sessions")
            .join(format!("{}.json", source.session_id))
    } else if source.path.is_dir() {
        source.path.join(format!("{}.json", source.session_id))
    } else {
        source.path.clone()
    };

    let bytes = fs::read(&file_path)?;
    let doc: Value = serde_json::from_slice(&bytes)?;
    let Value::Object(doc_map) = doc else {
        return Err(AdapterError::InvalidPayload {
            message: "session file must be a JSON object".to_owned(),
        });
    };

    let metadata = doc_map.get("metadata").and_then(Value::as_object);
    let session_id = metadata
        .and_then(|m| m.get("id"))
        .and_then(Value::as_str)
        .unwrap_or(&source.session_id)
        .to_owned();

    let cwd = metadata
        .and_then(|m| m.get("workspace"))
        .and_then(Value::as_str)
        .map(PathBuf::from)
        .unwrap_or_else(|| source.cwd.clone());

    let started = metadata
        .and_then(|m| m.get("created_at"))
        .and_then(Value::as_str)
        .and_then(crate::claude::time::parse)
        .unwrap_or_default();

    let ended = metadata
        .and_then(|m| m.get("updated_at"))
        .and_then(Value::as_str)
        .and_then(crate::claude::time::parse);

    let model = metadata
        .and_then(|m| m.get("model"))
        .and_then(Value::as_str)
        .map(str::to_owned);

    let journal_entries = doc_map
        .get("journal")
        .and_then(Value::as_object)
        .and_then(|j| j.get("entries"))
        .and_then(Value::as_array);

    let items_to_process: Vec<&Value> = if let Some(entries) = journal_entries {
        let leaf_id = doc_map
            .get("journal")
            .and_then(Value::as_object)
            .and_then(|j| j.get("leaf_id"))
            .and_then(Value::as_str)
            .or_else(|| doc_map.get("leaf_id").and_then(Value::as_str));

        if let Some(leaf) = leaf_id {
            let mut entry_map = BTreeMap::new();
            for entry in entries {
                if let Some(id) = entry.get("id").and_then(Value::as_str) {
                    entry_map.insert(id, entry);
                }
            }
            let mut branch = Vec::new();
            let mut curr = Some(leaf);
            let mut visited = BTreeSet::new();
            while let Some(id) = curr {
                if !visited.insert(id) {
                    break;
                }
                if let Some(entry) = entry_map.get(id) {
                    branch.push(*entry);
                    curr = entry.get("parent_id").and_then(Value::as_str);
                } else {
                    break;
                }
            }
            branch.reverse();
            branch
        } else {
            entries.iter().collect()
        }
    } else if let Some(messages) = doc_map.get("messages").and_then(Value::as_array) {
        messages.iter().collect()
    } else {
        Vec::new()
    };

    let mut total_records = 0_u64;
    let mut skipped_records = 0_u64;
    let mut turns: Vec<Turn> = Vec::new();
    let mut pending_calls: BTreeMap<String, (usize, usize)> = BTreeMap::new();

    for item in items_to_process {
        if let Some(kind) = item.get("kind").and_then(Value::as_str) {
            match kind {
                "compaction" | "branch_summary" | "system" => {
                    total_records = total_records.saturating_add(1);
                }
                "user" => {
                    total_records = total_records.saturating_add(1);
                    let prompt = item
                        .get("text")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_owned();
                    turns.push(Turn {
                        prompt,
                        model: model.clone(),
                        stopped_by: StopReason::Unknown,
                        ..Turn::default()
                    });
                }
                "assistant" => {
                    total_records = total_records.saturating_add(1);
                }
                "message" => {
                    total_records = total_records.saturating_add(1);
                    if let Some(msg) = item.get("message") {
                        process_message(
                            msg,
                            &model,
                            &mut turns,
                            &mut pending_calls,
                            &mut total_records,
                            &mut skipped_records,
                        );
                    }
                }
                _ => {
                    total_records = total_records.saturating_add(1);
                    skipped_records = skipped_records.saturating_add(1);
                }
            }
        } else if item.get("role").is_some() {
            total_records = total_records.saturating_add(1);
            process_message(
                item,
                &model,
                &mut turns,
                &mut pending_calls,
                &mut total_records,
                &mut skipped_records,
            );
        } else {
            total_records = total_records.saturating_add(1);
            skipped_records = skipped_records.saturating_add(1);
        }
    }

    let partial = total_records > 0 && skipped_records.saturating_mul(5) > total_records;

    let mut session = Session::new(Host::Deepseek);
    session.agent_home = source.agent_home.clone();
    session.session_id = session_id;
    session.repo_key = source.repo_key.clone();
    session.cwd = cwd;
    session.started = started;
    session.ended = ended;
    session.turns = turns;
    session.usage = Usage::default();
    session.partial = partial;
    session.skipped_records = skipped_records;
    session.total_records = total_records;

    Ok(session)
}

fn process_message(
    message: &Value,
    model: &Option<String>,
    turns: &mut Vec<Turn>,
    pending_calls: &mut BTreeMap<String, (usize, usize)>,
    total_records: &mut u64,
    skipped_records: &mut u64,
) {
    let Some(role) = message.get("role").and_then(Value::as_str) else {
        return;
    };

    if role == "system" || role == "developer" {
        return;
    }

    let Some(content) = message.get("content") else {
        return;
    };

    if let Some(text) = content.as_str() {
        *total_records = total_records.saturating_add(1);
        if role == "user" {
            turns.push(Turn {
                prompt: text.to_owned(),
                model: model.clone(),
                stopped_by: StopReason::Unknown,
                ..Turn::default()
            });
        }
        return;
    }

    let Some(blocks) = content.as_array() else {
        *total_records = total_records.saturating_add(1);
        *skipped_records = skipped_records.saturating_add(1);
        return;
    };

    if role == "user" {
        let mut user_texts = Vec::new();
        for block in blocks {
            *total_records = total_records.saturating_add(1);
            let Some(btype) = block.get("type").and_then(Value::as_str) else {
                *skipped_records = skipped_records.saturating_add(1);
                continue;
            };
            match btype {
                "text" => {
                    if let Some(text) = block.get("text").and_then(Value::as_str) {
                        user_texts.push(text);
                    }
                }
                "tool_result" => {
                    let call_id = block
                        .get("tool_use_id")
                        .or_else(|| block.get("id"))
                        .and_then(Value::as_str)
                        .unwrap_or_default();
                    let is_error = block
                        .get("is_error")
                        .and_then(Value::as_bool)
                        .unwrap_or(false);
                    let content = block.get("content").unwrap_or(&Value::Null);
                    let output_digest = crate::digest::output_digest(content);

                    let exit_code =
                        if let Some(code) = content.get("exit_code").and_then(Value::as_i64) {
                            i32::try_from(code).ok()
                        } else if is_error {
                            crate::claude::map::parse_exit(&crate::digest::output_text(content))
                        } else {
                            None
                        };

                    if let Some(&(turn_idx, call_idx)) = pending_calls.get(call_id) {
                        if let Some(t) = turns.get_mut(turn_idx) {
                            if let Some(c) = t.calls.get_mut(call_idx) {
                                c.output_digest = output_digest;
                                c.is_error = is_error;
                                if exit_code.is_some() {
                                    c.exit_code = exit_code;
                                }
                            }
                        }
                    }
                }
                "thinking" => {}
                _ => {
                    *skipped_records = skipped_records.saturating_add(1);
                }
            }
        }
        if !user_texts.is_empty() {
            let prompt = user_texts.join("\n");
            turns.push(Turn {
                prompt,
                model: model.clone(),
                stopped_by: StopReason::Unknown,
                ..Turn::default()
            });
        }
    } else if role == "assistant" {
        for block in blocks {
            *total_records = total_records.saturating_add(1);
            let Some(btype) = block.get("type").and_then(Value::as_str) else {
                *skipped_records = skipped_records.saturating_add(1);
                continue;
            };
            match btype {
                "text" | "thinking" => {}
                "tool_use" => {
                    let id = block
                        .get("id")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_owned();
                    let name = block
                        .get("name")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_owned();
                    let input = block.get("input").unwrap_or(&Value::Null);

                    let args_digest = if matches!(
                        name.as_str(),
                        "exec_shell" | "bash" | "Bash" | "shell" | "shell_command" | "exec_command"
                    ) {
                        crate::digest::args_digest("bash", input)
                    } else {
                        crate::digest::args_digest(&name, input)
                    };

                    let call = Call {
                        tool: name,
                        args_digest,
                        ..Call::default()
                    };

                    if turns.is_empty() {
                        turns.push(Turn {
                            model: model.clone(),
                            stopped_by: StopReason::Unknown,
                            ..Turn::default()
                        });
                    }
                    let turn_idx = turns.len().saturating_sub(1);
                    if let Some(turn) = turns.last_mut() {
                        let call_idx = turn.calls.len();
                        turn.calls.push(call);
                        if !id.is_empty() {
                            pending_calls.insert(id, (turn_idx, call_idx));
                        }
                    }
                }
                _ => {
                    *skipped_records = skipped_records.saturating_add(1);
                }
            }
        }
    }
}
