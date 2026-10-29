# Claude Code and Codex: to-verify items (SPEC 18)

Checked 2026-10-29 against the official docs and local installs: Claude Code 2.1.312,
codex-cli 0.164.0. Local probes ran in a scratch directory with a temporary `CODEX_HOME` and a
`--settings` file for Claude Code; the real `~/.claude` and `~/.codex` were not modified.

## Claude Code

| Item | Result | Evidence |
|---|---|---|
| Project-settings hook trust rule | **Closed.** Interactive sessions hold back hooks from every settings file, including `~/.claude/settings.json`, until the workspace trust dialog is accepted for the folder (or a parent). `-p` and SDK sessions treat the folder as trusted. tokenimum installs to user settings, so `tkm init` prints no trust step; a first interactive run in a new folder still needs the trust dialog. | [hooks, Workspace trust](https://code.claude.com/docs/en/hooks#workspace-trust) |
| `Stop` payload fields | **Closed.** Common fields (`session_id`, `prompt_id`, `transcript_path`, `cwd`, `permission_mode`, `effort`, `hook_event_name`) plus `stop_hook_active`, `last_assistant_message`, `background_tasks`, `session_crons`. | [hooks, Stop input](https://code.claude.com/docs/en/hooks#stop-input); probe dump `fixtures/claude/hooks/Stop.json` |
| `SessionEnd` payload fields | **Closed.** Common fields plus `reason` (`clear`, `resume`, `logout`, `prompt_input_exit`, `other`). Hooks share a 1.5 s budget (raised by a per-hook `timeout`, up to 60 s). JSON output, including `systemMessage`, is discarded. | [hooks, SessionEnd](https://code.claude.com/docs/en/hooks#sessionend); `fixtures/claude/hooks/SessionEnd.json` |
| Permissions deny syntax | **Closed.** Rules are `Tool` or `Tool(specifier)`. A bare tool name (or `Tool(*)`) as a deny rule removes the tool from Claude's context. MCP: `mcp__<server>` or `mcp__<server>__*` (whole server), `mcp__<server>__<tool>` (one tool); `mcp__` rules with parentheses are skipped from settings files. Deny wins over allow. `<repo>/.claude/settings.local.json` is the local project scope. | [permissions, rule syntax](https://code.claude.com/docs/en/permissions#permission-rule-syntax), [MCP rules](https://code.claude.com/docs/en/permissions#mcp) |

Further facts the adapter depends on (same pages, same date):

- Command handlers accept `args`; with `args`, `command` is spawned directly (exec form, no shell).
  `timeout` is in seconds; default 600 s, lowered to 30 s on `UserPromptSubmit`.
- `additionalContext`, `systemMessage` and plain stdout are each capped at 10,000 characters.
  Over the cap Claude Code saves the text to a file and passes a 2,000-character preview, so the
  adapter truncates to 10,000 characters itself.
- `additionalContext` on `Stop` is "feedback that continues the conversation". tokenimum never
  continues a turn, so the Claude adapter never injects on `Stop` or `SessionEnd`; a notice
  (`systemMessage`) is allowed on `Stop`, nothing on `SessionEnd`.
- `PostToolUseFailure` carries `tool_name`, `tool_input`, `tool_use_id`, `error`, `is_interrupt`,
  `duration_ms` and no `tool_response` (probe: `fixtures/claude/hooks/PostToolUseFailure-bash.json`).
- `SessionStart` carries `source` (`startup`, `resume`, `clear`, `compact`, `fork`) and sometimes
  `model`; the probe payload had neither `prompt_id` nor `permission_mode`.

## Codex

| Item | Result | Evidence |
|---|---|---|
| Hooks under `codex exec` | **Closed (documented), partly probed.** `codex exec --help` lists `--dangerously-bypass-hook-trust` ("Run enabled hooks without requiring persisted hook trust for this invocation"), and the hooks page describes it for one-off automation, so `exec` runs trusted hooks. Local probe: an untrusted `hooks.json` in a temporary `CODEX_HOME` produced 0 hook invocations under `codex exec`. A trusted-hook probe was not run: it needs either the bypass flag (tokenimum never uses it) or an interactive `/hooks` trust. Consequence: delegated Codex runs fire tokenimum hooks only after the person trusted them; otherwise they fall back to idle-fold. | [Codex hooks, Review and trust hooks](https://developers.openai.com/codex/hooks) (redirects to learn.chatgpt.com/docs/hooks); `codex exec --help` (0.164.0) |
| Tool deny key in `config.toml` / `rules/`; MCP tool filters | **Closed: no general deny key.** The config reference has `mcp_servers.<id>.enabled_tools` / `disabled_tools` ("Deny list applied after `enabled_tools`"), the same pair under `plugins.<plugin>.mcp_servers.<server>`, `tools.web_search`, `tools.view_image` and `tool_suggest.disabled_tools`. `rules.prefix_rules[].decision = "prompt" \| "forbidden"` exists only in managed `requirements.toml` and matches shell command prefixes, not tools. No key removes a built-in tool. Codex pruning stays **report-only** (ADR 0001 item 8); MCP `disabled_tools` is a possible later surface. | [Codex config reference](https://developers.openai.com/codex/config-reference) (redirects to learn.chatgpt.com/docs/config-file/config-reference) |

Further facts the adapter depends on (same pages, same date):

- Handler fields: `type`, `command` (a shell string; there is no `args`), `timeout` (seconds,
  default 600; `SessionEnd`/`Interrupt` default 1 s, max 3 s), `statusMessage`, `async`,
  `additionalContextLimit` (approximate tokens, default 2,500; `0` = unlimited), `commandWindows`.
- Trust state lives in `config.toml` as `[hooks.state."<path>:<event_snake_case>:<i>:<j>"]` with
  `trusted_hash = "sha256:..."`; the event segment is snake_case (`user_prompt_submit`,
  `pre_tool_use`), seen in a local `config.toml`. tokenimum never writes it.
- Common input: `session_id`, `transcript_path` (string or null; format "not a stable
  interface"), `cwd`, `hook_event_name`, `model`; `turn_id` on turn-scoped events;
  `permission_mode` on all events but `SessionEnd`, `PreCompact`, `PostCompact`.
- `PostToolUse` fires for shell commands with non-zero exit as well (no separate failure event);
  `apply_patch` arrives as `tool_name: "apply_patch"`.
- `Stop` requires JSON or empty stdout on exit 0 (plain text is invalid). `SessionEnd` output is
  advisory and does not steer Codex; `reason` is always `other`; it also fires after 30 minutes
  idle.
- Rollouts: `sessions/YYYY/MM/DD/rollout-<ts>-<uuid>.jsonl`, first record `session_meta` with
  `originator`, `source`, `cli_version`, `cwd`. The local `~/.codex/sessions` holds 296
  `.jsonl.zst` files (zstd frames, checked with `file`).
