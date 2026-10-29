# agy, Cursor, Kimi Code, Codewhale: to-verify items (SPEC 18)

Checked 2026-10-29. agy 1.4.9 is installed locally and was probed; Cursor, Kimi Code and
Codewhale are not installed, so their items are closed from official docs and, where docs are
silent or wrong, from the upstream source at the commit named below. Anything still open uses the
safe fallback: no injection on that surface, report-only pruning, idle-fold for missing events.

## agy (Antigravity CLI 1.4.9)

Local probe: `agy -p` in scratch workspaces with a project `<ws>/.agents/hooks.json` whose handlers
wrote stdin to files. The real `~/.gemini/config/hooks.json` was not modified (agy itself wrote
its normal conversation files under `~/.gemini/antigravity-cli/`). Redacted dumps are in
`fixtures/agy/`. Docs: [hooks (CLI tab)](https://antigravity.google/docs/hooks?tab=cli),
[CLI settings](https://antigravity.google/docs/cli/settings),
[CLI permissions](https://antigravity.google/docs/cli/permissions).

| Item | Result | Evidence |
|---|---|---|
| Handler shape | **Closed.** Each event holds a flat list of handlers `{"type":"command","command":"...","timeout":N}`. The nested `{"matcher","hooks":[...]}` shape shown in the docs is rejected by 1.4.9: `invalid hook "probenested": command hook must specify 'command'`, and the whole file fails to load. `matcher` (regex on tool name) is accepted on a flat handler. | probe log `hooks.go:109`; `fixtures/agy/config/hooks.json` |
| Exit-code semantics | **Closed (probe).** Exit 1 with garbage stdout on `Stop`: agy logs `failed to call custom stop hook ... exit status 1` and the turn completes normally (fail-open). Docs do not describe exit codes. | probe log `stophooks.go:62` |
| No-op reply | **Closed (probe).** Both empty stdout and `{}` are accepted without any log entry. tokenimum keeps `{}` (`Host::noop_reply`). | probe |
| Inject shapes | **Closed (probe).** `SessionStart` → `{"injectSteps":[{"systemMessage":{"systemMessage":"..."}}]}` and `PreInvocation` → `{"injectSteps":[{"ephemeralMessage":"..."}]}` both reached the model (it repeated both code words). In the transcript they appear as `SYSTEM_MESSAGE` and `EPHEMERAL_MESSAGE` steps. | `fixtures/agy/brain/049b40e1-.../transcript_full.jsonl` |
| Notice field | **Closed: none.** No person-visible message field is documented for any event; tokenimum renders no notice on agy and logs instead. | hooks doc |
| Project-hook trust | **Closed (probe, print mode).** `<ws>/.agents/hooks.json` loaded and ran under `agy -p` with no trust prompt (`loaded 2 named hooks from 2 hooks.json file(s)`). Docs only mention a `trustedWorkspaces` setting without linking it to hooks. tokenimum installs to the global file, so no trust step is printed. | probe log `hooks_manager.go:53` |
| Payload fields | **Closed (probe).** All events: `conversationId`, `workspacePaths`, `transcriptPath`, `artifactDirectoryPath`, `modelName`, `lastUserInput` (present on every event in 1.4.9, although undocumented). `PreInvocation`/`PostInvocation` add `invocationNum` (0-based per turn), `initialNumSteps`. `Stop` adds `executionNum`, `terminationReason` (e.g. `NO_TOOL_CALL`), `error`, `fullyIdle`. Env: `ANTIGRAVITY_CONVERSATION_ID`. | `fixtures/agy/hooks/*.json` |
| `PreToolUse`/`PostToolUse` | **Open.** Documented (`toolCall{name,args}`, `stepIdx`, `error`), but neither fired under `agy -p` in two probes that ran `read_file`, `write_file`, `edit_file` and `run_command`, with or without `matcher`. Fallback: `tool_done` is optional in `tkm doctor --probe`; calls come from the transcript at fold time. | probe |
| `transcriptPath` format | **Closed (probe).** `<home>/.gemini/antigravity-cli/brain/<conversationId>/.system_generated/logs/transcript_full.jsonl` (a sibling `transcript.jsonl` had identical content). One JSON object per step: `step_index`, `source` (`USER_EXPLICIT`, `MODEL`, `SYSTEM`, `SYSTEM_SDK`), `type` (`USER_INPUT`, `PLANNER_RESPONSE`, `GENERIC`, `SYSTEM_MESSAGE`, `EPHEMERAL_MESSAGE`, `ERROR_MESSAGE`, `CHECKPOINT`), `status` (`DONE`, `RUNNING`, `ERROR`, `INVALID`), `created_at` (RFC 3339, s), optional `content`, `thinking`, `error`, `media`, `tool_calls[{name,args}]`, and on `PLANNER_RESPONSE` `input_tokens`, `cache_read_tokens`, `output_tokens`. A `GENERIC` step is the result of the preceding call, in order; its `content` starts with `Created At:` / `Completed At:` lines; shell results contain `Command exited with code N.`. Stats over 216 local transcripts (counts only, no content read): 99.6% of `PLANNER_RESPONSE` steps have 0 or 1 tool call; the most used tools are `run_command`, `view_file`, `replace_file_content`, `write_to_file` (1.4.9 also emits `read_file`, `write_file`, `edit_file`). | `fixtures/agy/brain/` |
| Conversation DB blob schema | **Closed: not decoded.** `conversations/<uuid>.db` tables `trajectory_meta`, `steps` (`step_payload`, `metadata`, ... blobs), `gen_metadata`, `executor_metadata`, `parent_references`, `trajectory_metadata_blob`, `battle_mode_infos`. Blobs are undocumented protobuf. The reader uses the transcript JSONL and never opens the DB for content. | `fixtures/agy/conversations/schema.sql` |
| Permissions deny syntax | **Closed: user scope only.** `~/.gemini/antigravity-cli/settings.json` `{"permissions":{"allow":[],"ask":[],"deny":[...]}}`, rules `action(target)` with actions `read_file`, `write_file`, `read_url`, `execute_url`, `command(prefix\|regex:...\|*)`, `mcp(server/tool\|server/*\|*)`; deny > ask > allow. No project-scoped settings file is documented, and tokenimum prunes per repository, so agy pruning stays **report-only**. | [CLI settings](https://antigravity.google/docs/cli/settings), [CLI permissions](https://antigravity.google/docs/cli/permissions) |
| Gemini CLI hooks | **Closed: separate product.** Different events (`BeforeTool`, `AfterTool`, `BeforeAgent`, ...) and exit-code rules; not applied to agy. | [geminicli.com/docs/hooks](https://geminicli.com/docs/hooks/) |

## Cursor

Not installed locally. Docs: [hooks](https://cursor.com/docs/hooks),
[third-party hooks](https://cursor.com/docs/agent/third-party-hooks),
[CLI permissions](https://cursor.com/docs/cli/reference/permissions).

| Item | Result | Evidence |
|---|---|---|
| Hooks in `agent` / `agent -p` | **Closed (forum, unofficial).** No official matrix. Forum reports: the CLI fires `sessionStart`, `sessionEnd`, `preToolUse`/`postToolUse`, shell, MCP and file hooks; `beforeSubmitPrompt`, `afterAgentResponse` and `stop` do not fire under `-p` (staff: known gap). The CLI may send an empty `conversation_id`. Consequence: `prompt` and `turn_end` are optional in `tkm doctor --probe`; missing ones fall back to idle-fold. | [forum 169059](https://forum.cursor.com/t/cursor-cli-omits-beforesubmitprompt-afteragentresponse-and-stop-hooks-loses-token-usage-and-emits-inconsistent-generation-id-values/169059), [forum 148511](https://forum.cursor.com/t/cursor-cli-hooks/148511) |
| Default timeout | **Closed: undocumented** ("platform default"). The installer always sets `timeout` (seconds). | hooks doc |
| `sessionStart` injection | **Closed (docs).** Output `additional_context` is "added to the conversation's initial system context"; the hook is fire-and-forget (`continue` not enforced). Reliability in the CLI is unproven, so injections that matter are repeated on the next `postToolUse`. | hooks doc |
| Replies | **Closed (docs).** `postToolUse` and `postToolUseFailure` accept `additional_context`. `beforeSubmitPrompt` accepts only `continue`, `user_message`. `stop` accepts `followup_message` (auto-submitted; never used). `sessionEnd` has no output. Exit 0 uses JSON stdout; exit 2 blocks; other codes fail open unless `failClosed`. Empty stdout is not documented, so the no-op is `{}`. | hooks doc |
| Payload fields | **Closed (docs).** Common: `conversation_id`, `generation_id`, `model`, `hook_event_name`, `cursor_version`, `workspace_roots`, `user_email`, `transcript_path` (null when transcripts are off). `sessionStart`: `session_id`, `is_background_agent`, `composer_mode`. `beforeSubmitPrompt`: `prompt`, `attachments`. `postToolUse`: `tool_name`, `tool_input`, `tool_output`, `duration`. `postToolUseFailure`: `error_message`, `failure_type` (`error`, `timeout`, `permission_denied`), `duration`, `is_interrupt`. `stop`: `status` (`completed`, `aborted`, `error`), `loop_count`. `sessionEnd`: `session_id`, `reason`, `duration_ms`, `final_status`, `error_message`. Env: `CURSOR_PROJECT_DIR`, `CURSOR_VERSION`, `CURSOR_TRANSCRIPT_PATH`, `CLAUDE_PROJECT_DIR` (alias). | hooks doc |
| Transcript format and storage | **Partly closed (forum).** Docs say only "path to the main conversation transcript file". Forum: JSONL at `~/.cursor/projects/<encoded-cwd>/agent-transcripts/<conversation_id>/<conversation_id>.jsonl`, rows `{role, message:{content:[{type:"text",text}\|{type:"tool_use",name,input}]}}`, with no timestamps, usage or tool outputs. The reader parses this shape and marks the session `partial` when records do not match; the fixture is synthetic. | [forum 166592](https://forum.cursor.com/t/richer-agent-transcripts-lifecycle-data-for-observability-langfuse-stop-hooks/166592) |
| Claude Code hook loading | **Closed (docs).** With "Include Third-Party Plugins, Skills, and Other Configs" on (default), Cursor loads hooks from `.claude/settings.local.json`, `.claude/settings.json` and `~/.claude/settings.json`, maps event names (`UserPromptSubmit` → `beforeSubmitPrompt`, ...), and runs every matching hook from every source; no dedup is documented. A tokenimum Claude entry would therefore fire as `--host claude` inside Cursor. Handling: ADR 0020. | third-party hooks doc |
| Permissions | **Closed (docs).** `<repo>/.cursor/cli.json` and `~/.cursor/cli-config.json`: `{"permissions":{"allow":[],"deny":[]}}`, entries `Shell(cmd)`, `Read(glob)`, `Write(glob)`, `WebFetch(domain)`, `Mcp(server:tool)` with `*`; deny wins. | CLI permissions doc |
| Project hook trust | **Closed (docs).** "Project hooks run only in trusted workspaces." User hooks need no trust. | hooks doc |

## Kimi Code (kimi-cli 1.52.0)

Not installed locally. Docs: [hooks](https://raw.githubusercontent.com/MoonshotAI/kimi-cli/main/docs/en/customization/hooks.md),
[data locations](https://raw.githubusercontent.com/MoonshotAI/kimi-cli/main/docs/en/configuration/data-locations.md),
[config files](https://raw.githubusercontent.com/MoonshotAI/kimi-cli/main/docs/en/configuration/config-files.md).
Source checked at commit `9ab1286` (release 1.52.0, 2026-09-22).

| Item | Result | Evidence |
|---|---|---|
| Stdout injection per event | **Closed: not implemented.** Docs say exit-0 stdout "is added to context". The source never reads `HookResult.stdout` at any call site; stdout JSON is parsed only for `hookSpecificOutput.permissionDecision == "deny"`. Exit 2 blocks (effective on `PreToolUse`, `UserPromptSubmit`, `Stop`). tokenimum still prints plain text on `SessionStart` and `UserPromptSubmit` (harmless, and correct if Kimi implements the docs), never JSON and never exit 2. Facts reach Kimi through the fold-only instructions block until injection is proven. | `src/kimi_cli/hooks/runner.py`, `engine.py` |
| Project-level hooks | **Closed: not supported.** Hooks load only from the global `config.toml` (or `--config-file`). | `src/kimi_cli/config.py` |
| Plugin hooks | **Closed: not supported.** | `src/kimi_cli/plugin/` |
| Schema | **Closed.** `[[hooks]]` keys `event`, `command` (run with the shell, cwd = work dir), `matcher` (regex, default all), `timeout` (seconds, default 30, 1–600). Matching hooks run in parallel; identical `command` strings are deduplicated per event. | `src/kimi_cli/hooks/config.py` |
| Payloads | **Closed.** Common `hook_event_name`, `session_id` (UUID), `cwd`. `SessionStart`: `source`. `UserPromptSubmit`: `prompt`. `PostToolUse`: `tool_name`, `tool_input`, `tool_output` (first 2,000 chars), `tool_call_id`. `PostToolUseFailure` (Python exception only): `tool_name`, `tool_input`, `error`, `tool_call_id`. `Stop`: `stop_hook_active`. `SessionEnd`: `reason`. No env vars. | `src/kimi_cli/hooks/events.py` |
| Session files | **Closed (source).** `<KIMI_SHARE_DIR or ~/.kimi>/sessions/<md5hex(work_dir)>/<session_id>/` with `context.jsonl` (messages `role` user/assistant/tool, `tool_calls[{id,function{name,arguments}}]`, `tool_call_id`, plus `_system_prompt`, `_checkpoint`, `_usage{token_count}` records), `wire.jsonl` (`{"timestamp","message":{"type","payload"}}`; per-step usage in `StatusUpdate.token_usage{input_other,output,input_cache_read,input_cache_creation}`), `state.json`. The hash input is the unresolved absolute work-dir string. Fixtures are synthetic. | `src/kimi_cli/metadata.py`, `soul/context.py`, `wire/types.py` |
| Tool deny rules | **Closed: none.** Report-only. | config-files doc |

## Codewhale (formerly DeepSeek-TUI, 0.10.2)

Not installed locally. Docs: [HOOKS.md](https://github.com/Hmbown/DeepSeek-TUI/blob/main/docs/HOOKS.md),
[CONFIGURATION.md](https://github.com/Hmbown/DeepSeek-TUI/blob/main/docs/CONFIGURATION.md),
[RECEIPTS.md](https://github.com/Hmbown/DeepSeek-TUI/blob/main/docs/RECEIPTS.md) (the repository
now redirects to `codewhale-hq/Codewhale`).

| Item | Result | Evidence |
|---|---|---|
| Which harness is meant | **Closed, decision recorded, person may override.** DeepSeek now publishes an official CLI, DeepSeek Harness (`dsh`, npm `@deepseek-ai/dsh`, created 2026-08-13, developer preview). E5B targets Codewhale, as the SPEC says; `dsh` is a separate future adapter (follow-up). | [deepseek-ai/deepseek-harness](https://github.com/deepseek-ai/deepseek-harness); Codewhale `docs/INTEGRATIONS_DSH.md` |
| Session file format | **Closed (source).** One JSON document per session: `$CODEWHALE_HOME/sessions/<uuid>.json` with `schema_version`, `metadata{id,title,created_at,updated_at,total_tokens,model,workspace,mode,cost}`, `messages[{role,content:[{type:text\|thinking\|tool_use{id,name,input}\|tool_result{tool_use_id,content,is_error}}]}]`, `journal{entries[{id,parent_id,kind,message,created_at}],leaf_id}`, `turn_outcomes`. No per-turn usage in the file. The hook session id (`sess_xxxxxxxx`) is not the file id; correlate by workspace and time. Fixtures are synthetic. | `crates/tui/src/session_manager.rs`, `crates/runtime/src/session_tree.rs` |
| Hook config | **Closed.** `[[hooks.hooks]]` keys `event`, `command` (`sh -c`), `name`, `timeout_secs` (default 30), `background`, `continue_on_error` (default true), `condition`. `[hooks] default_timeout_secs` overrides every hook. Project `<ws>/.codewhale/hooks.toml` needs trust plus `/hooks approve <digest>`. `codewhale exec` fires hooks only with `--hooks`, and then only `tool_call_before` and `shell_env`. | HOOKS.md |
| Inputs | **Closed.** Env on all but `shell_env`: `DEEPSEEK_SESSION_ID` (= `CODEWHALE_SESSION_ID`), `DEEPSEEK_WORKSPACE`, `DEEPSEEK_MODEL`, `DEEPSEEK_MODE`; tool events add `DEEPSEEK_TOOL_NAME`, `DEEPSEEK_TOOL_ARGS` (10,000 bytes), `DEEPSEEK_TOOL_CALL_ID`, `DEEPSEEK_TOOL_RESULT`, `DEEPSEEK_TOOL_SUCCESS`, `DEEPSEEK_TOOL_EXIT_CODE`, `DEEPSEEK_TOOL_STATUS`. Stdin JSON on `message_submit` (`text`, `session_id`, `workspace`, ...), `turn_end` (`turn_id`, `status`, `duration_ms`, `usage{input_tokens,output_tokens,prompt_cache_hit_tokens,prompt_cache_miss_tokens,...}`), and shell `tool_call_after`; none on `session_start`, `session_end`, `tool_call_before`. | HOOKS.md, `crates/tui/src/hooks/executor.rs` |
| Replies | **Closed.** Only `tool_call_before` can inject: `{"decision":"allow","additionalContext":"..."}`, 2,000 chars per field, 8,000 per call, appended to the tool result as `[hook context]`. Empty stdout is allow / no change on every event. Exit 2 denies. | HOOKS.md |
| Tool deny rules | **Closed: user scope only.** `~/.codewhale/permissions.toml` `[[rules]]` (`tool`, `command`, `path`, `workspace`, `action`); no project file. Report-only. | CONFIGURATION.md |
