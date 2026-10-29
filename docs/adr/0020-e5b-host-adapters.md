# ADR 0020: agy, Cursor, Kimi Code and Codewhale adapters (E5B)

Date: 2026-10-29. Status: accepted.

## Context

E5B adds four `HostAdapter`s to `tkm-hosts`, following ADR 0011. Verified host facts and the
evidence for each are in `docs/verified/e5b-hosts.md`. Their config formats differ from the
Claude Code / Codex matcher-group format, so the shared writer gains format-neutral helpers. E4
(`tkm-hook`, `tkmd`) needs two facts per host that the trait does not carry: which environment
variables to forward, and which events `tkm doctor --probe` should expect.

## Decisions

### Modules and names

| Host | `Host` | Module | Adapter | `--host` |
|---|---|---|---|---|
| agy | `Agy` | `tkm_hosts::agy` | `AgyAdapter` | `agy` |
| Cursor | `Cursor` | `tkm_hosts::cursor` | `CursorAdapter` | `cursor` |
| Kimi Code | `Kimi` | `tkm_hosts::kimi` | `KimiAdapter` | `kimi` |
| Codewhale | `Deepseek` | `tkm_hosts::codewhale` | `CodewhaleAdapter` | `deepseek` |

Each module also exports `pub const EXPECTED_EVENTS: &[probe::ExpectedEvent]` and
`pub const ENV_MARKERS: &[&str]`.

### Shared additions (additive)

- `tkm_hosts::probe`: `ExpectedEvent { native, kind, required, note }` and
  `expected_events(host) -> &'static [ExpectedEvent]` for every host (Claude Code and Codex lists
  live in `probe.rs`). `required: false` marks an event that is documented but not observed in a
  local probe or reported missing in some mode; `tkm doctor --probe` reports it but its absence
  only switches that event to idle-fold.
- `tkm_hosts::env_markers(host) -> &'static [&'static str]`: the only environment variables
  `tkm-hook` copies into `Request.env` (empty for Claude Code, Codex, Kimi).
- `tkm_hosts::writer`: `install_text_file` / `uninstall_text_file`, the format-neutral core of
  `install_hooks` / `uninstall_hooks` (backup, created-file marker, symlink refusal, atomic write,
  re-read and retry, dry run). JSON helpers on top: `install_member` / `uninstall_member` (one
  top-level member owned by name, used by agy) and `install_flat_hooks` / `uninstall_flat_hooks`
  (handler objects directly in `<container>.<event>` arrays, owned per handler by
  `owns_handler`, with seed members such as `"version": 1` inserted only when absent, used by
  Cursor). TOML writers (Kimi, Codewhale) use `toml_edit` inside the text-file core.
  `install_hooks` and `uninstall_hooks` keep their behaviour and tests.

### agy

- Install: user `<agent_home>/config/hooks.json` (`agent_home` = `~/.gemini`), project
  `<repo>/.agents/hooks.json`. tokenimum owns the top-level member `tokenimum` and nothing else.
  Handlers use the flat shape verified on 1.4.9:
  `{"type":"command","command":"tkm-hook --host agy --event <kind>","timeout":<s>}`.
  Events: `SessionStart` (session_start), `PreInvocation` (prompt), `PostToolUse` (tool_done,
  `matcher` absent), `Stop` (turn_end). Timeouts: 3 s for prompt, 1 s otherwise.
- `PreInvocation` fires once per model invocation; only `invocationNum == 0` maps to `prompt`,
  later invocations map to `None` and render the no-op.
- Replies: inject on `SessionStart` as `systemMessage` step, on `PreInvocation` (first
  invocation) as `ephemeralMessage` step; capped at 10,000 chars. No notice field exists, so
  notices are dropped. Everything else renders `{}`. Never `decision`, `terminationBehavior`.
- No session end: idle-fold. Reader reads the transcript JSONL (`transcriptPath`); a `.db` or
  directory source is resolved to
  `<agent_home>/antigravity-cli/brain/<session_id>/.system_generated/logs/transcript_full.jsonl`.
  The conversation DB is never opened for content (protobuf blobs, undocumented).
- Pruning: report-only (deny rules exist only in user-scope settings).

### Cursor

- Install: user `<agent_home>/hooks.json` (`agent_home` = `~/.cursor`), project
  `<repo>/.cursor/hooks.json`; `{"version":1,"hooks":{"<event>":[{"command":"tkm-hook --host cursor --event <kind>","timeout":<s>}]}}`.
  Events: `sessionStart`, `beforeSubmitPrompt`, `postToolUse`, `postToolUseFailure`, `stop`,
  `sessionEnd`.
- Replies: `{"additional_context":"..."}` on `sessionStart`, `postToolUse`,
  `postToolUseFailure` (cap 10,000 chars); `{}` elsewhere. `beforeSubmitPrompt` opens the turn
  and records the prompt but never injects; the daemon delivers the block on the next event that
  can inject (E4). Never `continue`, `user_message`, `followup_message`, `permission`.
- Double registration: Cursor runs Claude Code hooks from `~/.claude/settings.json` and the
  repository's `.claude/settings{,.local}.json`. `cursor::is_cursor_payload(&Value)` recognises a
  Cursor payload (it carries `cursor_version`); a `--host claude` request with a Cursor payload
  must be ignored by the daemon (E4), because the `--host cursor` handler already delivers the
  same event. `install` checks `<agent_home>/../.claude/settings.json` (and the repository files
  for project scope) for tokenimum Claude handlers and, if found, adds an instruction to the
  report explaining the duplicate and the setting that controls it.
- Reader: `transcript_path` JSONL in the forum-reported shape; no usage or outputs. Pruning:
  `SettingsDenyList { path: ".cursor/cli.json", key: "permissions.deny" }`.

### Kimi Code

- Install: `<agent_home>/config.toml` (`agent_home` = `KIMI_SHARE_DIR` or `~/.kimi`), user scope
  only; project scope returns `AdapterError::Unsupported`. One `[[hooks]]` table per event with
  `event`, `command = "tkm-hook --host kimi --event <kind>"`, `timeout`, and a leading
  `# tokenimum` comment line. A table is tokenimum's when its `command` passes `owns_handler`.
  Events: `SessionStart`, `UserPromptSubmit`, `PostToolUse`, `PostToolUseFailure`, `Stop`,
  `SessionEnd`.
- Replies: plain text stdout on `SessionStart` and `UserPromptSubmit` (cap 10,000 chars), empty
  elsewhere. Kimi 1.52.0 ignores it (verified in source); the instructions-file block is the
  working injection path, and the probe metadata marks injection unverified.
- Reader: session directory `<share>/sessions/<md5hex(cwd)>/<session_id>/`, `context.jsonl` for
  turns and calls, `wire.jsonl` for timestamps and usage. `kimi::session_dir` computes the path
  with an in-crate MD5 (RFC 1321, safe Rust, test vectors), so no new dependency. Pruning:
  report-only.

### Codewhale

- Install: `<agent_home>/config.toml` (`agent_home` = `CODEWHALE_HOME` or `~/.codewhale`), user
  scope only. One `[[hooks.hooks]]` table per event with `event`, `command`,
  `name = "tokenimum-<event>"`, `timeout_secs`, `continue_on_error = true` (never false: a failed
  foreground hook with `false` denies the tool call). `[hooks]` is created when absent; its
  `enabled` and `default_timeout_secs` are never changed; if `enabled = false` the report says
  so. Events: `session_start`, `message_submit` (prompt), `tool_call_before` (tool_before),
  `tool_call_after` (tool_done), `turn_end`, `session_end`.
- Inputs come from env markers (`DEEPSEEK_*`, `CODEWHALE_SESSION_ID`, `CODEWHALE_TOOL_CALL_ID`)
  and stdin where present; `map_event` accepts `Value::Null` payloads.
- Replies: only `tool_call_before` injects, `{"decision":"allow","additionalContext":"..."}`,
  capped at 2,000 chars. Empty stdout everywhere else; `message_submit` never rewrites the prompt.
- Reader: `sessions/<uuid>.json`, active branch of `journal` (walk `parent_id` from `leaf_id`)
  when present, else `messages`. Pruning: report-only.
