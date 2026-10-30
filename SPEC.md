# tokenimum: specification

Status: draft 2, 2026-10-29. Specification only; nothing is built yet.
Product name: **tokenimum**. CLI: `tkm`. Hook client: `tkm-hook`. Daemon: `tkmd`.

Design origin: microsoft/TokenEconomizer (TE), Apache-2.0. See `NOTICE`.

---

## 1. Summary

tokenimum makes coding agents cheaper in the repositories a person uses often. It learns from
finished sessions and uses that experience to cut repeated waste in later sessions. It replicates
the three mechanisms of TokenEconomizer: repository facts, tool pruning, and a failure-loop guard.
It adds what TE lacks:

1. **Speed.** A Rust hook client and a long-running Rust daemon. Hooks answer in under 3 ms.
2. **Decision models instead of chat models.** Every judgment (is this turn stuck, is this fact
   worth keeping, is this context relevant) goes to an OpenRouter Decisions model through one
   client with a priority chain. No `claude -p` side calls, no agent account quota.
3. **Broad harness support.** Claude Code, Codex, Antigravity (`agy`), opencode, Cursor (agent and
   CLI), Kimi Code, a DeepSeek harness, and Pi, all first-class (section 6).
4. **Standalone first.** tokenimum works on its own. superorchestrator and otherskill are optional
   adapters, detected and enabled only when present (section 9).
5. **Easy to use, monitor and customize.** One-command install, a status view, a live terminal
   monitor, an optional local web dashboard, a layered config with presets, per-mechanism
   switches, and a plain-language explanation of every action (section 11).
6. **Entirely safe Rust.** Every crate has `#![forbid(unsafe_code)]`; dependencies are audited
   (section 13).

## 2. Goals and non-goals

### Goals

- G1. Lower mean cost per session in repeat repositories without lowering task success.
- G2. Hook overhead small enough to ignore: p50 under 3 ms, p99 under 10 ms per hook.
- G3. One learning store per repository, shared across harnesses and accounts where a fact is
  harness-neutral.
- G4. Every model judgment through the Decisions API; total model spend under 1% of the agent spend
  it saves.
- G5. Fail open everywhere: tokenimum down, network down or model error means the agent runs
  unchanged.
- G6. Every action visible, explainable from stored evidence in plain language, and reversible.
- G7. Works with no other tool installed. Integrations with superorchestrator and otherskill add
  value when present and never become requirements.
- G8. No `unsafe` code in tokenimum's own crates.

### Non-goals

- Output compression of tool results (rtk, caveman and headroom already do it).
- Changing agent models, weights or system prompts beyond injected context and settings files.
- Copilot CLI support (TE covers it; not in our harness set).
- Generating prose with a model. Where text is needed, tokenimum uses templates.

## 3. Glossary

| Term | Meaning |
|---|---|
| Harness | A coding agent CLI or app: Claude Code, Codex, agy, opencode, Cursor, Kimi Code, DeepSeek harness, Pi. |
| Host adapter | The tokenimum code for one harness: hook payload mapping, reply shapes, session log reader, settings writer, installer. |
| Integration level | How a harness is attached: `hooks` (native command hooks), `shim` (minimal generated plugin that forwards to `tkm-hook`), or `fold-only` (no live hooks; idle-fold of session logs plus wrapper or skill). |
| Agent home | The config directory of one harness install (`~/.claude`, `~/.codex`, `~/.cursor`, `~/.kimi`, `~/.pi/agent`, ...). |
| Account home | An agent home owned by one superorchestrator account (`~/.codex-1`, `~/.agy-1/.gemini`, ...). Only relevant with the superorchestrator adapter. |
| Repository key | Stable id of a repository: the normalized `origin` remote URL; else the main checkout path. |
| Session | One agent conversation, from start to end. |
| Turn | The agent's work on one user prompt (from prompt submit to stop). |
| Fold | Post-session processing of one finished session's log. |
| Idle-fold | A fold triggered because a session log has been idle for a set time, used when a harness has no session-end event. |
| Fact | A short learned statement about a repository ("run `python -m pytest`, not `pytest`"). TE calls these micro-skills. |
| Envelope | The p90 tool-call count of this repository's turns that ended well, times a margin. |
| Decision | One request to the OpenRouter Decisions API: a state plus typed questions. |
| Digest | A compact, redacted text summary of a turn or session, built by code, sent as a decision's state. |
| Delegated run | A session started by an orchestrator (superorchestrator), not by a person. |
| Action | Anything tokenimum does that the agent or person can notice: inject, redirect, notice, deny-list write, restore, escalation event. Every action is an `events` row with an explanation. |

## 4. How TokenEconomizer works, and what tokenimum keeps

### 4.1 TE flow (replicated)

```
SessionStart ─► inject active facts (cached render) + notices ─► agent works
UserPromptSubmit ─► open turn state
PostToolUse (every tool call) ─► fold call into TurnGuard
        ├─ free gate: repeated failure or dense errors after N calls?
        │     └─ yes ─► judge (detached model call) ─► stuck? ─► redirect as additionalContext
        └─ hard backstop: stuck AND turn > 2.5 × envelope ─► redirect without judge
        (another stuck segment after a redirect ─► notice "press Esc"; never stops the turn)
Stop ─► close turn; turn that ended well feeds the envelope
SessionEnd ─► detached fold:
        ├─ facts: observe commands ─► candidate facts ─► lifecycle (promote / retire)
        ├─ tool pruning: count unused tools over prompts and sessions ─► deny list (after idle wait)
        └─ ledger: price the session (cache-aware) incl. plugin overhead
```

### 4.2 What tokenimum changes in that flow

| Step | TE | tokenimum |
|---|---|---|
| Process model | new `python3` per hook | Rust `tkm-hook` client, Rust `tkmd` daemon over a Unix socket |
| Turn state | JSON files read and written per tool call | in daemon memory, flushed to SQLite every 5 s and at turn end |
| Judge | `claude -p --model haiku`, free-text verdict | Decisions `noul` questions, one request |
| Redirect text | model-written or rule template | `choice` over a redirect library, filled from a template |
| Fact proposer | Haiku proposes facts as prose | code extracts candidates; Decisions `noul` filters them |
| Fact relevance at injection | all active facts, capped | `score` per item against the current prompt, shared token budget |
| Headless "still stuck" | printed notice nobody reads | structured event to the orchestrator adapter when present; else stored event plus notice |
| Harnesses | Claude Code, Codex, Copilot, Pi | Claude Code, Codex, agy, opencode, Cursor, Kimi Code, DeepSeek harness, Pi |
| Install | one agent home | `tkm init`: every detected agent home; optional superorchestrator account-home mode |
| Spend accounting | own `ledger.jsonl` | own ledger in SQLite; optional report to superorchestrator usage |
| Monitoring | CLI reports | `tkm status`, `tkm top` TUI, optional web dashboard, `tkm why` |

### 4.3 TE parameters kept as defaults

| Setting | Default | Meaning |
|---|---|---|
| `facts.promote_confirmations` | 3 | sessions that must confirm a candidate before it is injected |
| `facts.recency_decay` | 0.8 | per-session decay of a fact's score |
| `facts.contradiction_weight` | 5.0 | weight of a contradiction against a confirmation |
| `facts.fire_gate` | 0.40 | inject only if the fact's failure occurs or is prevented in at least 40% of sessions that exercise it |
| `facts.fire_min_sessions` | 4 | minimum exercised sessions for the fire gate |
| `facts.max_facts` / `facts.max_tokens` | 4 / 150 | cap of the injected fact block |
| `prune.evidence_prompts` | 10 | prompts a tool must go unused |
| `prune.min_sessions` | 2 | sessions those prompts must span |
| `prune.sessions_any` | 5 | or this many sessions of any length |
| `prune.quiet_minutes` | 60 | idle time before a deny list is written (prompt cache expiry) |
| `loop.min_turns` | 20 | tool calls before a turn can be judged stuck |
| `loop.gate_min_turns` | 18 | tool calls before the free gate can wake the judge |
| `loop.gate_error_outputs` | 8 | error outputs that wake the judge without an exact repeat |
| `loop.edit_churn` | 4 | edits to one file that count as churn |
| `loop.stagnation` | 0.20 | share of failures that repeat an earlier one |
| `loop.envelope_pct` / `loop.envelope_margin` | 90 / 1.25 | envelope = p90 of good turns × 1.25 |
| `loop.envelope_min_samples` | 5 | good turns before the guard acts in a repository |
| `loop.backstop_mult` | 2.5 | redirect without the judge past 2.5 × envelope when stuck by rule |
| `loop.redirects_per_turn` | 1 | redirects before escalation |
| `loop.level` | `default` | presets `conservative` / `default` / `aggressive` as in TE |

TE's decision thresholds (for example `judge_confidence = 0.7`) were tuned for a chat-model judge.
tokenimum re-calibrates every decision threshold per model (section 8.5).

## 5. Architecture

```
 ┌──────── harness process (Claude Code / Codex / agy / Cursor / Kimi / DeepSeek harness) ────────┐
 │ hook event JSON ─► tkm-hook --host <h> (Rust, static, ~1 ms) ──Unix socket──┐                  │
 │ ◄── host-shaped reply JSON (context / notice / {}) ◄─────────────────────────┤                  │
 └──────────────────────────────────────────────────────────────────────────────┼──────────────────┘
 ┌──────── opencode / Pi (TS-only plugin APIs) ─────────┐                       │
 │ generated shim (≤ 40 lines, no logic) ─► tkm-hook ───┼───────────────────────┤
 └──────────────────────────────────────────────────────┘                       │
 ┌──────── fold-only harnesses ────────┐                                        │
 │ session log files ◄── idle-fold ────┼──────── read-only ─────────────────────┤
 └─────────────────────────────────────┘                                        ▼
 ┌────────────────────────────────────── tkmd (Rust, tokio) ──────────────────────────────────────┐
 │ router ─► host adapters (claude | codex | agy | opencode | cursor | kimi | deepseek | pi)        │
 │ live layer:   TurnGuard per active turn (memory) ─► gate ─► decision queue                       │
 │ inject layer: fact + skill + memory candidates ─► relevance scoring ─► budget                    │
 │ fold workers: session log readers ─► observe ─► facts lifecycle / prune / ledger                 │
 │ decision client: OpenRouter Decisions, model chain, cache, rate limit, budget, ledger            │
 │ store: SQLite (WAL) at ~/.local/share/tokenimum/tokenimum.db                                    │
 │ integrations (optional, auto-detected): superorchestrator adapter, otherskill adapter            │
 │ monitor API: local socket (TUI, CLI) + optional HTTP on 127.0.0.1 (web dashboard)                │
 └──────────────────────────────────────────────────────────────────────────────────────────────────┘
```

### 5.1 Components

| Component | Language | Responsibility |
|---|---|---|
| `tkm-hook` | Rust, static binary, no async runtime | Read stdin, add host name and env markers, send one framed request to the socket, print the reply. Timeout 2 ms connect, hook-specific read timeout (section 6). On any failure print the host's no-op reply and exit 0. |
| `tkmd` | Rust, tokio | Daemon: all state, all logic, all network. Started by a systemd user unit (Linux) or launchd agent (macOS); `tkm-hook` starts it on first miss (detached), without waiting. |
| `tkm` | Rust CLI | `init`, `status`, `top`, `why`, `facts`, `prune`, `config`, `doctor`, `calibrate`, `replay`, `export`, `dashboard`, `uninstall`. Talks to `tkmd`, or opens the DB read-only when the daemon is down. |
| opencode shim | generated TypeScript, ≤ 40 lines | Documented exception to the Rust-only rule (section 13.3). Forwards plugin events to `tkm-hook --host opencode` and applies the reply. No logic. |
| Pi shim | generated TypeScript, ≤ 40 lines | Same exception, for Pi extensions. Uses `pi.exec` to call `tkm-hook --host pi`. |

### 5.2 Crate layout

```
tokenimum/
  Cargo.toml                 (workspace; [workspace.lints.rust] unsafe_code = "forbid")
  deny.toml                  (cargo-deny policy)
  crates/
    tkm-proto/               request/reply types, framing (length-prefixed JSON)
    tkm-hook/                the hook client binary
    tkm-core/                mechanisms: facts, prune, loopguard, ledger (no I/O, no network)
    tkm-hosts/               host adapters: payload mapping, reply shapes, log readers, settings writers
    tkm-decide/              OpenRouter Decisions client, model chain, cache, calibration
    tkm-store/               SQLite schema, migrations, queries
    tkm-integrations/        optional adapters: superorchestrator, otherskill (detection + bridges)
    tkm-config/              layered config, presets, env overrides, provenance
    tkm-ui/                  ratatui monitor (`tkm top`) and web dashboard (feature `web`)
    tkmd/                    daemon binary
    tkm-cli/                 `tkm` binary
  shims/                     templates for the generated opencode and Pi shims (not built by cargo)
  fixtures/                  recorded sessions per host (redacted) for tests and replay
  SPEC.md
```

Every crate root starts with `#![forbid(unsafe_code)]`. The workspace also sets
`unsafe_code = "forbid"` in `[workspace.lints.rust]`, and every crate has `[lints] workspace = true`.

`tkm-core` has no I/O and no network. Every mechanism is a pure function of (state, event) to
(new state, actions), so it is tested by replaying fixtures.

### 5.3 Libraries (check current docs before coding; no API from memory)

tokio, serde/serde_json, rusqlite (bundled, WAL), reqwest (rustls), tracing, clap, ratatui with
crossterm, axum (feature `web`), regex, globset, toml_edit (Codex, Kimi, DeepSeek harness configs),
directories. Pin versions in `Cargo.lock`. Every dependency passes the policy in section 13.2.

## 6. Harness support

All eight harnesses are first-class: each has a host adapter, a log reader, an installer and
fixture-based tests. They differ only in integration level, decided by what the harness exposes.

Facts below were researched from official docs (and local installs where present) on
2026-10-29. Items marked **to verify** are not confirmed; each carries the doc link to check.

### 6.1 Integration level per harness

| Harness | Level | Mechanism | Version seen |
|---|---|---|---|
| Claude Code | `hooks` | settings `hooks` entries, `type: "command"` | 2.1.312 (local) |
| Codex | `hooks` | `hooks.json` or `config.toml` hooks; per-hook trust | 0.164.0 (local) |
| Antigravity (`agy`) | `hooks` | `~/.gemini/config/hooks.json`, `<repo>/.agents/hooks.json` | 1.4.9 (local) |
| Cursor (agent, CLI) | `hooks` | `~/.cursor/hooks.json`, `<repo>/.cursor/hooks.json` | not installed locally |
| Kimi Code | `hooks` (beta in Kimi) | `[[hooks]]` in `~/.kimi/config.toml` | not installed locally |
| DeepSeek harness | `hooks` | Codewhale (formerly DeepSeek-TUI) `[hooks]` TOML; see 6.9 | not installed locally |
| opencode | `shim` | TypeScript plugin API only (Bun) | 1.18.35 (local) |
| Pi | `shim` | TypeScript extensions only | not installed locally |

Fallback for every harness: if the hook or shim path is unavailable, disabled by the person, or
fails `tkm doctor`, the adapter drops to `fold-only` (section 6.12).

### 6.2 Events tokenimum handles

| tokenimum event | Claude Code | Codex | agy | Cursor | Kimi Code | DeepSeek harness (Codewhale) | opencode (shim) | Pi (shim) | Read timeout |
|---|---|---|---|---|---|---|---|---|---|
| `session_start` | `SessionStart` | `SessionStart` | `SessionStart` | `sessionStart` | `SessionStart` | `session_start` (env only, no stdin, cannot inject) | `event` `session.created` | `session_start` | 300 ms |
| `prompt` | `UserPromptSubmit` | `UserPromptSubmit` | `PreInvocation` (`lastUserInput`, agy ≥ 1.4.7) | `beforeSubmitPrompt` (cannot inject) | `UserPromptSubmit` | `message_submit` | `chat.message` | `before_agent_start` | 1,500 ms (relevance scoring) |
| `tool_done` | `PostToolUse`, `PostToolUseFailure` | `PostToolUse` | `PostToolUse` | `postToolUse`, `postToolUseFailure` | `PostToolUse`, `PostToolUseFailure` | `tool_call_after` | `tool.execute.after` | `tool_result` | 5 ms |
| `turn_end` | `Stop` | `Stop` | `Stop` | `stop` | `Stop` | `turn_end` | `event` `session.idle` | `agent_end` (not `turn_end`, ADR 0025) | 5 ms |
| `session_end` | `SessionEnd` | `SessionEnd` | **missing**: idle-fold | `sessionEnd` | `SessionEnd` | `session_end` (env only) | `event` `session.deleted`; else idle-fold | `session_shutdown` | 5 ms |

Rules:

- `tool_done` never waits on the network. A decision started by one tool call is delivered on a
  later tool call (as TE does), or dropped if the turn ends first.
- `prompt` may wait for relevance scoring up to its timeout. On timeout it injects the cached
  default block (top facts by score, no per-prompt scoring).
- Where `prompt` cannot inject (Cursor `beforeSubmitPrompt`, Codewhale `session_start`), the inject
  layer moves the block to the next event that can: Cursor `sessionStart` or the first
  `postToolUse`; Codewhale `tool_call_before` `additionalContext`.
- If a harness lacks `session_end`, `tkmd` folds a session when its log is idle for 30 minutes, or
  at the next `session_start` in the same repository (TE does the same for unseen ends).
- The harness's own hook timeout must exceed tokenimum's read timeout. Installers set it (for
  example `timeout: 3` s for `prompt`, `1` s for the others where the unit is s).

### 6.3 Reply shapes

The host adapter maps a tokenimum action to the harness format:

| Harness | Inject context | Notice to person | Nothing |
|---|---|---|---|
| Claude Code | `{"hookSpecificOutput":{"hookEventName":"<Event>","additionalContext":"..."}}` (cap 10,000 chars); not on `Stop` (would continue the turn) or `SessionEnd` (output discarded) | `{"systemMessage":"..."}` | exit 0, empty stdout |
| Codex | same shape as Claude Code; allowed on `SessionStart`, `UserPromptSubmit`, `PreToolUse`, `PostToolUse`, `SubagentStart`; not `Stop`/`SessionEnd`. Default cap ~2,500 tokens, raised with handler `additionalContextLimit` | `systemMessage` | exit 0, empty stdout |
| agy | `{"injectSteps":[{"systemMessage":{"systemMessage":"..."}}]}` on `SessionStart`; `{"injectSteps":[{"ephemeralMessage":"..."}]}` on the first `PreInvocation` (both verified by local probe 2026-10-29) | none exists; tokenimum logs instead | `{}` (verified; empty stdout also accepted) |
| Cursor | `{"additional_context":"..."}` on `sessionStart`, `postToolUse`, `postToolUseFailure` | `user_message` exists only on block/deny replies; tokenimum uses no notice on Cursor and logs instead | exit 0, `{}` |
| Kimi Code | exit 0 with plain stdout on `SessionStart`/`UserPromptSubmit` as documented; kimi-cli 1.52.0 source ignores hook stdout (verified 2026-10-29), so the instructions-file block is the working path | no notice field documented | exit 0, empty stdout |
| DeepSeek harness (Codewhale) | `tool_call_before` → `{"decision":"allow","additionalContext":"..."}` (appended to the tool result as `[hook context]`, cap 2,000 chars per field); `message_submit` can rewrite prompt text but tokenimum never rewrites prompts | none | exit 0, empty stdout |
| opencode (shim) | push a text part in `chat.message` output, or push to `output.system` in `experimental.chat.system.transform` | toast via `client.tui.showToast({body:{message,variant:"info"}})`, verified 2026-10-29 (`docs/verified/opencode-pi.md`) | return without mutation |
| Pi (shim) | `before_agent_start` returns `{message:{customType:"tokenimum",content,display:false}}`; redirects via `pi.sendMessage(..., {deliverAs:"steer"})` | `ctx.ui.notify(message, "info")`, verified 2026-10-29 (`docs/verified/opencode-pi.md`) | return `undefined` |

tokenimum never uses blocking replies (`decision: "block"`, exit 2, `continue: false`,
`followup_message`, `terminationBehavior`). It never stops a turn.

### 6.4 Session log readers

| Harness | Source | Notes |
|---|---|---|
| Claude Code | `<home>/projects/<cwd-slug>/<session>.jsonl` | `transcript_path` in hook payload; written asynchronously, may lag the current turn |
| Codex | `<CODEX_HOME>/sessions/YYYY/MM/DD/rollout-<ts>-<uuid>.jsonl`; older files `.jsonl.zst` | first line `session_meta`; `transcript_path` may be null; zstd decoding needed for old files |
| agy | hook `transcriptPath` = `<home>/.gemini/antigravity-cli/brain/<id>/.system_generated/logs/transcript_full.jsonl` (one JSON step per line, with token counts) | verified 2026-10-29; the `conversations/<uuid>.db` blobs are undocumented protobuf and are not read |
| Cursor | `transcript_path` in hook payload (also env `CURSOR_TRANSCRIPT_PATH`) | undocumented; forum: `~/.cursor/projects/<cwd>/agent-transcripts/<id>/<id>.jsonl`, rows `{role,message:{content}}` without usage or outputs |
| Kimi Code | `~/.kimi/sessions/<md5(workdir)>/<session-id>/{context.jsonl,wire.jsonl,state.json}` | base overridable with `KIMI_SHARE_DIR`; source: https://raw.githubusercontent.com/MoonshotAI/kimi-cli/main/docs/en/configuration/data-locations.md |
| DeepSeek harness (Codewhale) | `$CODEWHALE_HOME/sessions/<uuid>.json` | one JSON document (`metadata`, `messages`, `journal`); verified from source 2026-10-29 |
| opencode | SQLite `~/.local/share/opencode/opencode.db` (tables `session`, `message`, `part`, ...; path from `opencode db path`); legacy `~/.local/share/opencode/storage/` | open read-only; `opencode export <id>` is the fallback |
| Pi | `~/.pi/agent/sessions/--<cwd-encoded>--/<timestamp>_<session-id>.jsonl`, header `{type:"session",version:3,...}` | source: https://github.com/badlogic/pi-mono/blob/main/packages/coding-agent/docs/session-format.md |

Each reader produces one normalized shape:

```rust
struct Session { host, agent_home, session_id, repo_key, cwd, started, ended,
                 turns: Vec<Turn>, tool_catalog: Vec<ToolDef>, usage: Usage }
struct Turn    { prompt, calls: Vec<Call>, stopped_by: StopReason, usage: Usage }
struct Call    { tool, args_digest, output_digest, exit_code, is_error, duration_ms,
                 edits: Vec<FileEdit> }
```

Readers open foreign SQLite files with `SQLITE_OPEN_READ_ONLY` and never write to them. Unknown
record types are skipped and counted; a reader that skips more than 20% of records marks the
session `partial` and the fold uses it for the ledger only.

### 6.5 Claude Code

- Config: `~/.claude/settings.json`, `<repo>/.claude/settings.json`, `<repo>/.claude/settings.local.json`;
  entries merge across levels. Source: https://code.claude.com/docs/en/hooks
- Registration: `{"hooks":{"<Event>":[{"matcher":"*","hooks":[{"type":"command","command":"tkm-hook","args":["--host","claude"],"timeout":3}]}]}}`.
  With `args`, the command runs in exec form without a shell.
- Payload fields used: `session_id`, `transcript_path`, `cwd`, `hook_event_name`, `prompt`,
  `tool_name`, `tool_input`, `tool_response`, `duration_ms`, `agent_id`.
- Timeouts: command default 600 s; `SessionEnd` hooks share a 1.5 s budget, so `session_end` only
  records an end marker and the fold runs in `tkmd`.
- Tool pruning target: `<repo>/.claude/settings.local.json` `permissions.deny`; a bare tool name
  removes the tool from context, MCP rules are `mcp__<server>__<tool>` (verified 2026-10-29,
  `docs/verified/claude-codex.md`).
- Trust: interactive sessions run hooks from any settings file only after the workspace trust
  dialog for the folder; `-p`/SDK sessions treat the folder as trusted (verified 2026-10-29).
- `Stop` adds `stop_hook_active`, `last_assistant_message`, `background_tasks`, `session_crons`;
  `SessionEnd` adds `reason` and discards JSON output (verified 2026-10-29).

### 6.6 Codex

- Config: `~/.codex/hooks.json`, `~/.codex/config.toml`, `<repo>/.codex/hooks.json`,
  `<repo>/.codex/config.toml`, plugin `hooks/hooks.json`. `[features] hooks = false` disables.
  Source: https://developers.openai.com/codex/hooks
- Registration shape matches Claude Code; handler fields `type`, `command`, `timeout`,
  `statusMessage`, `async`, `additionalContextLimit`.
- Events: `SessionStart`, `SessionEnd`, `PreToolUse`, `PermissionRequest`, `PostToolUse`,
  `PreCompact`, `PostCompact`, `UserPromptSubmit`, `SubagentStart`, `SubagentStop`, `Stop`,
  `Interrupt`. `SessionEnd` default timeout 1 s, max 3 s.
- Trust: non-managed hooks are skipped until reviewed. Trust is stored per hook hash in
  `config.toml` (`[hooks.state."<path>:<event_snake_case>:<i>:<j>"] trusted_hash = "sha256:..."`); the person
  trusts with `/hooks` in the TUI. A changed hook needs trust again. `tkm init` prints the exact
  step and never writes `trusted_hash` itself. `codex exec --dangerously-bypass-hook-trust` exists;
  tokenimum never adds it.
- Handlers have no `args`: `command` is a shell string.
- `codex exec` runs trusted hooks and skips untrusted ones (docs plus local probe, 2026-10-29,
  `docs/verified/claude-codex.md`). Delegated runs whose hooks are not trusted fall back to
  idle-fold.
- Tool pruning: Codex has no deny key for built-in tools (only MCP `disabled_tools`; verified
  2026-10-29). Report only.

### 6.7 Antigravity (`agy`)

- Config: global `~/.gemini/config/hooks.json`; project `<repo>/.agents/hooks.json`; plugin
  `hooks.json`. Schema: top-level object `name → {enabled?, <Event>: [...]}`. Source:
  https://antigravity.google/docs/hooks?tab=cli
- tokenimum registers its own top-level name `tokenimum`, beside existing names such as
  `otherskill`, so entries never collide. Each event holds a flat handler list
  `{"type":"command","command":...,"timeout":...}`; agy 1.4.9 rejects the nested
  `{"matcher","hooks":[...]}` shape shown in the docs (probe 2026-10-29).
- Events: `SessionStart`, `PreToolUse`, `PostToolUse`, `PreInvocation`, `PostInvocation`, `Stop`.
  No session end: idle-fold.
- Payload: `conversationId`, `workspacePaths`, `transcriptPath`, `artifactDirectoryPath`,
  `modelName`, `lastUserInput`; `PreInvocation` adds `invocationNum` (0 = first of the turn);
  `PostToolUse` adds `toolCall`, `stepIdx`, `error`. `PreToolUse`/`PostToolUse` did not fire under
  `agy -p` 1.4.9, so `tool_done` is optional for agy.
- Timeout unit: s, default 30.
- Exit codes: non-zero fails open (probe). Project hooks loaded without a trust prompt under
  `agy -p` (probe). Details: `docs/verified/e5b-hosts.md`.
- Tool pruning: deny rules `action(target)` exist only in user-scope
  `~/.gemini/antigravity-cli/settings.json` `permissions` (https://antigravity.google/docs/cli/permissions);
  tokenimum prunes per repository, so agy stays report only.
- Gemini CLI hooks (https://geminicli.com/docs/hooks/) are a separate product; not assumed to apply.

### 6.8 Cursor (agent and CLI)

- Config: `~/.cursor/hooks.json`, `<repo>/.cursor/hooks.json`, `/etc/cursor/hooks.json`. Shape:
  `{"version":1,"hooks":{"<event>":[{"command":"tkm-hook --host cursor","timeout":3}]}}`.
  Source: https://cursor.com/docs/hooks
- Events used: `sessionStart`, `beforeSubmitPrompt`, `postToolUse`, `postToolUseFailure`, `stop`,
  `sessionEnd`.
- `beforeSubmitPrompt` replies only `{continue, user_message}`: it cannot inject. tokenimum uses it
  to open the turn and record the prompt, then injects on the next `postToolUse`.
- `sessionStart` is fire-and-forget; its `additional_context` joins the initial system context
  (docs). Blocks that matter are repeated on the next `postToolUse`.
- Which hooks fire in the CLI: docs are silent; forum reports say `beforeSubmitPrompt` and `stop`
  do not fire under `agent -p` (https://forum.cursor.com/t/cursor-cli-hooks/148511). `tkm doctor` runs a probe session
  and records which events arrived; missing events switch that event to idle-fold.
- Default hook timeout undocumented: installer always sets `timeout`.
- Tool pruning target: `<repo>/.cursor/cli.json` `permissions.deny` (syntax `Shell(cmd)`,
  `Mcp(server:tool)`, ...). Source: https://cursor.com/docs/cli/reference/permissions
- Project hooks run only in trusted workspaces.
- Cursor also loads Claude Code hooks by default (https://cursor.com/docs/agent/third-party-hooks,
  verified). The installer warns about it and the daemon ignores `--host claude` requests that
  carry a Cursor payload (ADR 0020).

### 6.9 Kimi Code

- Config: `[[hooks]]` tables in `~/.kimi/config.toml` with `event`, `command`, `matcher`,
  `timeout` (s, default 30). Hooks are marked Beta upstream. Source:
  https://raw.githubusercontent.com/MoonshotAI/kimi-cli/main/docs/en/customization/hooks.md
- Events: all five tokenimum events map directly (table 6.2).
- Payload: `session_id`, `cwd`, `hook_event_name`, `prompt`, `tool_name`, `tool_input`,
  `tool_output`, `source`, `reason`.
- Reply: exit 0 with stdout adds context; exit 2 blocks (tokenimum never uses it). Hooks of one
  event run in parallel; identical commands are deduplicated.
- Project-level and plugin-registered hooks: not supported (source, 2026-10-29). Despite the docs,
  kimi-cli 1.52.0 does not add hook stdout to the context.
- Tool pruning: no allow/deny rules documented
  (https://raw.githubusercontent.com/MoonshotAI/kimi-cli/main/docs/en/configuration/config-files.md):
  report only.

### 6.10 DeepSeek harness

- DeepSeek publishes no official coding CLI as of 2026-10-29. The adapter targets the most used
  community harness, **Codewhale** (formerly DeepSeek-TUI, binary `codewhale`, legacy `deepseek`),
  https://github.com/Hmbown/DeepSeek-TUI, which states it is not affiliated with DeepSeek.
  DeepSeek has since published an official CLI, DeepSeek Harness (`dsh`, developer preview,
  https://github.com/deepseek-ai/deepseek-harness); it is a separate future adapter.
- Config: global `~/.codewhale/config.toml` `[hooks]` with `[[hooks.hooks]]`; legacy
  `~/.deepseek/config.toml`; project `<repo>/.codewhale/hooks.toml` with `[[hooks]]` (needs
  workspace trust and `/hooks approve <digest>`). Source: https://github.com/Hmbown/DeepSeek-TUI/blob/main/docs/HOOKS.md
- Events used: `session_start`, `message_submit`, `tool_call_before`, `tool_call_after`,
  `turn_end`, `session_end`, `session_idle`. The current upstream docs state that
  `codewhale exec` fires no hooks by default; `--hooks` opts into engine-side `tool_call_before`
  and `shell_env` only. Use interactive session observations for the other events.
- Input: env `DEEPSEEK_SESSION_ID`, `DEEPSEEK_WORKSPACE`, `DEEPSEEK_TOOL_NAME`,
  `DEEPSEEK_TOOL_EXIT_CODE`, ...; stdin JSON for `message_submit`, `turn_end` and tool events.
  `tkm-hook --host deepseek` reads both.
- Injection only through `tool_call_before` `additionalContext` (cap 2,000 chars per field,
  8,000 per call).
- Timeout `timeout_secs` default 30; `continue_on_error` default true.
- Tool pruning: no deny list; report only.

### 6.11 opencode and Pi (shim level)

opencode plugins and Pi extensions are TypeScript modules loaded in-process. Neither harness
offers config-declared command hooks (opencode: https://opencode.ai/docs/plugins/; Pi:
https://github.com/badlogic/pi-mono/blob/main/packages/coding-agent/docs/extensions.md). A pure
Rust live integration is therefore not possible. tokenimum has two modes for each:

1. **Rust-only mode (default when the person sets `shims = false`):** no plugin. `fold-only`
   integration (section 6.12): read-only fold of `opencode.db` or Pi session JSONL, facts and
   prune proposals still learned; injection through a generated `AGENTS.md` block or a skill; no
   live loop guard.
2. **Shim mode (default otherwise):** `tkm init` generates one minimal TypeScript file from a
   template in `shims/`. Constraints:
   - at most 40 lines, no npm dependencies, no logic beyond: serialize event, spawn
     `tkm-hook --host <opencode|pi>` with a hard timeout, apply the reply;
   - opencode spawns with Bun `$` (as the existing local otherskill and rtk plugins do); Pi uses
     `pi.exec(cmd, args, {timeout})`;
   - the file carries a header naming tokenimum, its version and a content hash; `tkm doctor`
     verifies the hash and `tkm uninstall` removes only files with a matching header;
   - install paths: opencode `~/.config/opencode/plugins/tokenimum.ts` (or
     `<repo>/.opencode/plugins/`); Pi `~/.pi/agent/extensions/tokenimum.ts` (or
     `<repo>/.pi/extensions/`, which needs Pi project trust).

   This is the **one documented exception** to the Rust-only rule (section 13.3); it is open
   question 1.

opencode mapping: `session_start` = `event` `session.created`; `prompt` = `chat.message`;
`tool_done` = `tool.execute.after`; `turn_end` = `event` `session.idle`; `session_end` = `event`
`session.deleted`, else idle-fold. Hook timeouts: none (checked 2026-10-29), so the shim's own spawn
timeout is the limit. `session.created` is published when `opencode run` creates its session (source
read, not run at runtime), so the daemon still creates a session on the first event for an unknown
id and keeps the idle-fold fallback. Wire contract: `docs/adr/0025-shim-protocol.md`.

opencode pruning target: `opencode.json` `permission` object (`"<tool>": "deny"`, patterns
supported). Source: https://opencode.ai/docs/permissions/. MCP tools are keyed `<server>_<tool>` (both
parts sanitized to `[a-zA-Z0-9_-]`), verified 2026-10-29; a `<server>_*` wildcard key is expected from
the documented `*` wildcard but was not run.

Pi mapping: `session_start`; `prompt` = `before_agent_start`; `tool_done` = `tool_result`;
`turn_end` = `agent_end` (Pi's `turn_end` fires per model response, `agent_end` once per run; ADR 0025);
`session_end` = `session_shutdown` (the docs do not say whether async work is awaited there, so it is
best effort). Pi has no documented handler timeouts; the shim bounds `pi.exec`. Pi pruning target: settings `defaultTools`
(`-name` entries) in project settings. Source:
https://github.com/badlogic/pi-mono/blob/main/packages/coding-agent/docs/settings.md

### 6.12 Fold-only fallback

Used for a harness without hooks, with hooks disabled, or when `tkm doctor` finds events missing.

- **Idle-fold.** `tkmd` watches known session directories (inotify/FSEvents through the `notify`
  crate; a 60 s scan when watching is not possible). A session idle for 30 minutes is folded.
- **Injection without hooks.** Facts are written into a marked block in the repository's agent
  instructions file (`AGENTS.md`, or the harness's equivalent), between
  `<!-- tokenimum:begin -->` and `<!-- tokenimum:end -->`, only if the person enabled
  `inject.instructions_file`. The block is regenerated, never edited by hand, and the file is
  added to `.git/info/exclude` when tokenimum created it.
- **Skill integration.** A `tokenimum` skill (read-only commands: `tkm facts --brief`,
  `tkm why`) is installed into the harness's skill directory, so the agent can pull facts on
  demand.
- **Wrapper.** `tkm run <harness> [args...]` starts the harness with `TKM_SESSION_ID` and
  `TKM_HOST` in its environment, records start and end, and triggers an immediate fold on exit.
- No live loop guard in fold-only mode.

### 6.13 Settings writers and install steps per harness

| Harness | Hook install target | Format | Trust step printed by `tkm init` |
|---|---|---|---|
| Claude Code | `<home>/settings.json` `hooks` | JSON merge | none (user settings) |
| Codex | `<CODEX_HOME>/hooks.json` | JSON merge | run `codex`, then `/hooks`, trust tokenimum entries |
| agy | `<home>/.gemini/config/hooks.json`, top-level `tokenimum` | JSON merge | `/hooks` lists them; trust rule **to verify** |
| Cursor | `~/.cursor/hooks.json` | JSON merge | restart Cursor/agent; project hooks need workspace trust |
| Kimi Code | `~/.kimi/config.toml` `[[hooks]]` | `toml_edit`, comments kept | none documented |
| DeepSeek harness | `~/.codewhale/config.toml` `[[hooks.hooks]]` | `toml_edit` | project hooks: `/hooks approve` |
| opencode | `~/.config/opencode/plugins/tokenimum.ts` | generated shim | none (plugins auto-load) |
| Pi | `~/.pi/agent/extensions/tokenimum.ts` | generated shim | `/reload` in Pi; project scope needs project trust |

Writer rules: add only tokenimum's entries (tagged by command path `tkm-hook`, a JSON `name`, or a
TOML comment marker); keep every other entry (otherskill, rtk, mem-admission, ...); back up the
file to `<file>.tkm-bak` before the first write; refuse symlinks pointing out of the agent home;
idempotent re-runs.

## 7. Mechanisms

### 7.1 Repository facts

Flow per finished session:

1. **Observe** (code). Split shell commands into segments. Classify intent (test, interpreter,
   build/check, package tool, fetch, VCS probe) and form (spelling). Read outcome: error class,
   tests ran, exit code. Port TE `microskills/observe.py` with its `GENERIC` vocabulary. Commands
   rewritten by rtk (`rtk <cmd>`) are normalized back to `<cmd>` first.
2. **Extract candidates** (code, no model). Port TE's five families from `envfacts.py`:
   - form repair: a failing form followed by a working one (`pytest` fails, `python -m pytest` works);
   - boundary: every fetch to a host refused;
   - empty probe: VCS probes that always return empty;
   - argument repair: a test label that loads only once rewritten;
   - env value: a missing environment variable that a later run sets.
3. **Filter** (decision, batched: all candidates of the session in one request).

   | Question | Type | Use |
   |---|---|---|
   | `stable_<id>` | noul: "true = holds for this repository regardless of task; false = specific to this task" | drop below `t_low` |
   | `safe_<id>` | noul: "true = a harmless command or setting; false = destructive, secret-bearing, or completion advice" | drop below `t_high` |

4. **Contradiction check** (decision, batched): for each active fact the session exercised,
   noul `contradicts_<id>` against the session digest. Code evidence (the fact's command form
   failed where the fact says it works) always counts; the model verdict counts only above
   `t_high`.
5. **Lifecycle** (code). TE `lifecycle.py` rules: promote after 3 confirming sessions, recency
   decay 0.8, contradiction weight 5, hard kill on one clean contradiction for high-stakes facts,
   fire gate 40% over at least 4 sessions.
6. **Scope.** A fact is tagged `neutral` (command, env var, build order; shared across all
   harnesses) or `host:<name>` (mentions a harness's tool names; only that harness).

Facts are rendered by templates, never by a model:
`Run tests with \`{form}\` (\`{bad_form}\` fails here: {error_class}).`

With decisions disabled or over budget, steps 3 and 4 use code evidence only and candidates need
one extra confirmation.

### 7.2 Context budget and relevance (not in TE)

At `prompt`, `tkmd` builds one injection block from up to three candidate sources:

| Source | Candidates | Present when |
|---|---|---|
| tokenimum facts | active facts for this repository and harness | always |
| skills | otherskill's selection for this prompt | otherskill adapter enabled (section 9) |
| memory | pointer lines from agent memory indexes (`MEMORY.md`), when present | file exists and `inject.memory = true` |

Steps:

1. Drop exact and near duplicates (normalized text hash, then token Jaccard ≥ 0.8).
2. One decision request: score question `rel_<id>` per candidate, criteria
   `["irrelevant to this prompt", "might help", "likely needed", "required"]`, state = the prompt
   plus repository name.
3. Keep candidates with score ≥ 1.5 (of 0–3), sorted by score, until `inject.max_tokens`
   (default 600) is reached. Facts the fire gate marks as preventing a recurring failure are
   kept at score ≥ 1.0.
4. Cache the result by (prompt hash, candidate set hash) for 10 minutes.

Without otherskill, the block holds facts and memory pointers only. tokenimum does not drop context
the agent already holds. Hooks can only add context; pruning applies to what is injected and to
tools loaded in later sessions.

### 7.3 Tool and skill pruning

Port TE `toolprune` and `plugin/denylist.py`:

- Count per repository and harness: tools loaded (from the session catalog) and tools used, per
  prompt and per session.
- A tool unused over 10 prompts across at least 2 sessions, or over 5 sessions, becomes a deny-list
  candidate.
- Write the deny list only after 60 idle minutes in that repository (the prompt cache has expired),
  or after 24 hours at the latest.
- Modes: `off`, `report` (evidence only), `propose` (show the change, do not write; default), `apply`.
- Targets per harness:

  | Harness | Target | Status |
  |---|---|---|
  | Claude Code | `<repo>/.claude/settings.local.json` `permissions.deny` | verified surface |
  | Cursor | `<repo>/.cursor/cli.json` `permissions.deny` | verified surface |
  | opencode | `<repo>/opencode.json` `permission.<tool> = "deny"` | verified surface; MCP keys `<server>_<tool>` |
  | Pi | project settings `defaultTools` with `-name` | verified surface |
  | Codex | none for built-in tools (MCP `disabled_tools` only) | verified 2026-10-29; report only |
  | agy | none per repository (`settings.json` `permissions` is user scope) | verified 2026-10-29; report only |
  | Kimi Code | none documented | report only |
  | DeepSeek harness | none documented | report only |

- Add only tokenimum's own entries; never touch deny rules a person wrote; add new settings files
  to `.git/info/exclude`; refuse to edit a committed config or a symlink out of the repository.

**Protected set** (never pruned): every tool in TE's `PROTECTED_TOOLS` and `INTERACTIVE_PROTECTED`
(includes `Skill`, `ToolSearch`, `Agent`, `Task`), plus each harness's equivalents (opencode
`skill`, `task`; Pi `read`, `bash`, `edit`, `write`), `mcp__headroom__*`, anything in
`prune.protect`, and, when the superorchestrator adapter is enabled, `mcp__superorchestrator__*`.

**Delegation rule:** with the superorchestrator adapter enabled, a repository that has had any
delegated run in the last 30 days keeps pruning at `propose` at most.

**Skill usage evidence** (report only): count skills loaded (injected or listed) vs invoked, per
repository. Export as `tkm skills --json`; the otherskill adapter offers it as a ranking prior.
tokenimum never deletes or hides a skill.

### 7.4 Loop guard

Port TE `loopguard` (`TurnGuard`, `SpiralState`, `shell_edits`, `envelope`, `rules`):

1. Per `tool_done`: fold the call into the turn state (edits per file, files edited, error outputs,
   verification runs, repeat-failure ratio). Count shell edits (`sed -i`, `perl -pi`, redirects
   into source files, `tee`, inline scripts writing files, `patch`, `git apply`, `apply_patch`).
2. **Gate** (code): after `gate_min_turns` calls, open on a repeated failure or
   `gate_error_outputs` errors. The repository must have an envelope (5 good turns).
3. **Judge** (decision, async; at most one in flight per turn; at most one per 5 tool calls):

   | Question | Type | Criteria |
   |---|---|---|
   | `stuck` | noul | true: "repeats the same failing action or edit without a new approach"; false: "makes progress or tries new approaches" |
   | `progress` | noul | true: "the latest calls changed the failure or narrowed the cause"; false: "no change in outcome" |
   | `redirect` | choice | `reread_error`, `check_env`, `smaller_step`, `revert_last_edit`, `different_test`, `read_more_code`, `ask_user` |

   Verdict: stuck if `stuck ≥ t_stuck` and `progress ≤ t_progress`. Not stuck if
   `stuck ≤ t_low`. Otherwise wait for more calls.
4. **Backstop** (code, no model): rule-stuck and turn length > `backstop_mult` × envelope.
5. **Act.** First stuck verdict in a turn: redirect, delivered as injected context on the next
   `tool_done` (or the harness's next injectable event, table 6.2). Text from the redirect
   library, filled with the failing command and error line:
   `tokenimum: the last {n} calls repeated "{error_line}". Check the environment (imports,
   installed packages, working directory) before editing again.` Then start a fresh segment.
6. **Escalate.** A further stuck segment after the redirect budget:
   - interactive session: one notice with the harness's stop key (TE `hosts.stop_hint`, extended
     per harness);
   - delegated run with the superorchestrator adapter: event to superorchestrator (section 9.3);
   - delegated run without the adapter (for example `codex exec` under another script): stored
     `loop.escalation` event, visible in `tkm status`/`tkm top`, and an optional
     `loop.escalate_command` hook the person configures.
7. **Envelope.** At `turn_end`, a turn that stopped on its own, was not escalated, and had no failing
   check after its last edit feeds the envelope (last 500 good turns, p90 × 1.25).

tokenimum never stops a turn itself.

### 7.5 Ledger and savings

- Per session: tokens by class (input, cache write, cache read, output) from the session log;
  priced with a local price table (`pricing.toml`, refreshed weekly from OpenRouter
  `/api/v1/models` and harness vendor prices).
- Per decision: `usage.input_tokens`, `usage.cost`, model, latency, question names.
- tokenimum overhead = sum of decision cost for that session.
- **Savings estimate.** Per repository and harness, cost per session over time against a
  baseline: shadow sessions (no actions) and sessions before tokenimum's first action in that
  repository. Shown with sample counts and a confidence band; never shown as a single number
  without the sample size.
- Shadow sessions (section 8.5) are flagged so before/after comparisons stay honest.

## 8. Decision client (`tkm-decide`)

### 8.1 Endpoint

`POST https://openrouter.ai/api/alpha/decisions`, body `{model, state, questions, provider?,
session_id?, user?}`. Question types: `noul` (`criteria: {true, false}`), `choice` (`criteria`
keys = options), `score` (`criteria` = ordered array). Response `{id, model, provider, answers,
usage: {input_tokens, output_tokens, cost}}`. There is no `models` fallback array in the
Decisions API, so tokenimum implements the chain itself.

### 8.2 Model chain

Default chain, configurable in `~/.config/tokenimum/config.toml`:

| Priority | Model | Price (input / output) | Context | Why |
|---|---|---|---|---|
| 0 (if live) | `inception/mercury-decide:free` | free | 65,536 | requested primary; **returned 404 on 2026-10-29: "This model is unavailable for free. The paid version is available now"** |
| 1 | `inception/mercury-decide` | $0.02/M / free | 65,536 | primary in practice |
| 2 | `perplexity/pplx-decider-flash-v1.2` | $0.01/M / free | 262,144 | cheapest; fallback when Mercury is down |
| 3 | `typesafe/jev-1.14` | $0.042/M / free | 262,144 | quality fallback; reference model for calibration |

Other Decisions models on OpenRouter on 2026-10-29, for evaluation only: `liquid/d1.1`,
`liquid/d1-ultra`, `perplexity/pplx-decider-v1.2`, `nace-ai/drex-max`, `cloudflare/clef`,
`cloudflare/clef-flash`, `upstage/solar-decide-flash`, `microsoft/microsoft-decision-1`,
`respan/span-02`, `respan/span-01-lite:free` (behavior scoring over a conversation span; free).

Rules:

- Pin dated versions in config (`inception/mercury-decide` resolved to
  `inception/mercury-decide-20260930` in a probe); never `~...-latest` aliases.
- `tkm doctor` re-probes `:free` slugs daily and moves a free model to the front when it answers.
- On HTTP 404/5xx, timeout (1,500 ms), or invalid answer (missing key, probability outside 0–1),
  try the next model. After 3 failures in 5 minutes, skip a model for 15 minutes.
- Thresholds are stored per model version (section 8.5). Falling back to a model with no
  calibration means: log the answer, take no action.
- Decisions can be turned off entirely (`decide.enabled = false` or preset `offline`); every
  mechanism then runs its code-only path.

### 8.3 Probe result (2026-10-29)

One loop-guard digest (14 calls, `pytest` failing four times with `ImportError: no module named
app.db`, two `sed -i` edits) asked `stuck` (noul) and `redirect` (choice):

| Model | `stuck` | `redirect` | Input tokens | Cost | Wall time |
|---|---|---|---|---|---|
| `inception/mercury-decide:free` | 404 | — | — | — | 691 ms |
| `inception/mercury-decide` | 0.95 | `revert_last_edit` 0.93 | 246 | $0.0000049 | 944 ms |
| `typesafe/jev-1.14` | 0.78 | `check_env` 0.82 | 461 | $0.0000194 | 610 ms |

Both see the loop. They disagree on the redirect; for an `ImportError`, `check_env` is the better
answer. One sample proves nothing, but it shows why calibration and an agreement mode are required
before the redirect choice acts on its own.

### 8.4 Request rules

- **Digest, never raw logs.** State is built by code: prompt (first 500 characters), last 12 calls
  as `tool | args (≤120 chars) | exit | first error line`, counts (edits per file, error outputs,
  repeat ratio), test results. Hard cap 16 KB (well under the 32 KB long-context limit some
  models apply, and under Mercury's 65,536-token context).
- **Redaction** before any request: drop env values, tokens matching secret patterns (AWS, GitHub,
  OpenRouter, Anthropic, OpenAI, Moonshot, DeepSeek, Cursor, JWT, private keys), `.env` content,
  and paths outside the repository except the home prefix (replaced with `~`).
- **Batching:** all questions for one purpose in one request.
- **Provider preferences:** `data_collection: "deny"`, `allow_fallbacks: true`.
- **Cache:** key = hash(model, state, questions); TTL 10 minutes for relevance, 24 hours for fact
  filters.
- **Budget:** daily cap (default $0.50) and per-session cap (default $0.02). Over cap: code-only
  behaviour (rule gate, backstop, envfacts without the filter). `tkm top` shows spend against both
  caps.
- **Key:** `OPENROUTER_API_KEY`, read from env or `~/.config/tokenimum/openrouter.env` (mode 600).
  With the superorchestrator or otherskill adapter, tokenimum may reuse that tool's key file by
  path, if the person agrees during `tkm init`; it never copies the key.

### 8.5 Calibration

Decision thresholds (`t_stuck`, `t_progress`, `t_low`, `t_high`) differ per model. tokenimum ships
conservative defaults and learns per model version:

1. **Shadow mode** (default for the first 14 days or 200 judged turns): decisions are made and
   stored, no action is taken; TE's rule verdicts run beside them.
2. **Labels.** Turn outcomes label judged turns: a turn that later ended well after N more calls
   was not stuck; a turn the person interrupted, or that hit the backstop, was. Facts are labelled
   by later confirmation or contradiction.
3. **Fit.** `tkm calibrate` picks per model the threshold pair with the best precision at a
   minimum recall (default precision ≥ 0.9 for `stuck`), and writes it to the store.
4. **Agreement mode** (optional): ask two models in parallel (Mercury + Jev); act only when both
   pass threshold. Doubles cost (still under $0.0001 per judgment).
5. `tkm replay <fixtures>` re-runs recorded sessions against any model chain offline-cached, to
   compare models without live sessions.

Default thresholds before calibration: `t_stuck = 0.85`, `t_progress = 0.30`, `t_low = 0.30`,
`t_high = 0.80`.

## 9. Optional integrations

tokenimum is standalone. Two integrations are optional adapters in `tkm-integrations`.

### 9.1 Detection

At `tkmd` start, at `tkm init`, and every 10 minutes:

| Adapter | Detected when | Override |
|---|---|---|
| superorchestrator | `superorchestrator` on `PATH` **and** `~/.config/superorchestrator/` exists **and** `superorchestrator --version` answers within 1 s | `integrations.superorchestrator = "auto" \| "on" \| "off"` |
| otherskill | `otherskill` on `PATH` **and** a global or project otherskill config exists **and** `otherskill --help` answers within 1 s | `integrations.otherskill = "auto" \| "on" \| "off"` |

`auto` (default) enables an adapter only when detected. A detected adapter that later fails is
disabled for 15 minutes and logged; tokenimum keeps running. `tkm status` and `tkm doctor` show
each adapter's state and the reason.

### 9.2 Behaviour in the four combinations

| | otherskill absent | otherskill present |
|---|---|---|
| **superorchestrator absent** | **Standalone.** `tkm init` installs into every detected agent home. Injection block = facts + memory pointers. Loop escalation: notice (interactive) or stored event plus optional `loop.escalate_command` (headless). Spend only in tokenimum's ledger. Protected set excludes `mcp__superorchestrator__*` (absent anyway). Delegated runs are not detected; pruning default is `propose`. | **Standalone + skills.** As standalone, plus: injection block merges otherskill's per-prompt skill selection (dedupe + one shared budget); tokenimum exports skill usage evidence for otherskill's ranking; tokenimum reuses otherskill's OpenRouter key path only if the person agrees. Both keep their own hooks; tokenimum never edits otherskill's hook entries. |
| **superorchestrator present** | **Orchestrated.** As standalone, plus: optional account-home install mode; delegated-run detection; loop escalations sent to superorchestrator as events; decision spend reported to superorchestrator usage; `mcp__superorchestrator__*` protected; delegation rule for pruning. | **Full.** Both adapters active as described; this is the setup on the author's machine. |

In every combination, removing an integrated tool leaves tokenimum working: the adapter turns off
at the next detection pass, and the affected features fall back to the standalone behaviour.

### 9.3 superorchestrator adapter

**Account-home install mode.** `tkm init --superorchestrator` (or the matching prompt during
`tkm init` when detected) reads `~/.config/superorchestrator/accounts/*.toml` (`harness`, `home`)
and runs the per-harness installer (table 6.13) for each account home. Re-runs add new accounts
and leave existing entries. Accounts for harnesses outside table 6.1 are listed and skipped.

**Delegated-run detection.** A run is delegated when the hook environment carries a
superorchestrator run id. **Requirement on superorchestrator:** export `SUPERORCHESTRATOR_RUN_ID`
(and account name) into every spawned agent process. Until then, tokenimum matches the session's
cwd against `superorchestrator runs --json` worktree paths.

**Escalation event.** On escalation in a delegated run, `tkmd` emits:

```json
{"kind": "tokenimum.loop", "run_id": "...", "repo_key": "...", "turn_calls": 46,
 "envelope": 18, "stuck": 0.91, "progress": 0.12, "redirect_given": "check_env",
 "suggest": "redirect | cancel | reroute", "suggest_probs": {...}}
```

Delivery: superorchestrator CLI/MCP (`send_message` to the run for a redirect; the orchestrator
session decides `cancel_agent` or re-route). The exact channel is decided in M7 against the
superorchestrator API; tokenimum never cancels a run itself. `suggest` comes from one more
decision: choice `continue / redirect / cancel / reroute` with the digest and the run's routing
level as state.

**Spend.** Each decision's `usage.cost` is reported to superorchestrator usage under a synthetic
account `tokenimum-decisions` (format decided in M7), so headroom and routing see it. Agent-side
savings stay in tokenimum's ledger.

### 9.4 otherskill adapter

- **Skill candidates.** At `prompt`, call `otherskill select --json --agent <harness> --prompt
  <text>` (alias of `find --json`), with the prompt passed as a literal argument, never through a
  shell string. Its selected skills become candidates in the shared budget (section 7.2).
- **Avoid double injection.** When the otherskill adapter is on, tokenimum asks during
  `tkm init` whether to let tokenimum own the merged block. If yes, otherskill's own hook stays
  installed but tokenimum de-duplicates against it by skill name (open question 4 covers a cleaner
  hand-off). If no, tokenimum injects facts only and leaves skills to otherskill.
- **Ranking prior.** `tkm skills --json` exports loaded-vs-used evidence for otherskill to read.
- otherskill accepts only free models; tokenimum's chain is separate and may use paid models.

### 9.5 Repository identity across worktrees

Repository key = normalized `origin` URL, so worktrees (including superorchestrator run worktrees)
share facts, envelopes and evidence. Deny lists stay per checkout. TE's `paths.py` rules are
ported: parse `.git/config` directly, honour `gitdir:` files and `GIT_CEILING_DIRECTORIES`, stop at
`$HOME`.

## 10. Storage and configuration

### 10.1 Storage

SQLite at `~/.local/share/tokenimum/tokenimum.db` (WAL), one file for all repositories. Tables
(sketch):

| Table | Key | Content |
|---|---|---|
| `repos` | `repo_key` | display name, first/last seen, delegated-run count |
| `sessions` | `(host, agent_home, session_id)` | repo_key, cwd, start/end, fold status, shadow flag, integration level |
| `turns` | `(session, turn_idx)` | calls, ended-well flag, escalation, digest hash |
| `facts` | `(repo_key, fact_id)` | family, scope, text params, state (candidate/active/retired), score, confirmations, contradictions, fire stats |
| `tool_evidence` | `(repo_key, host, tool)` | prompts unused, sessions unused, last used |
| `denylist_applied` | `(checkout_path, host, tool)` | written at, restored flag |
| `envelopes` | `(repo_key, host)` | last 500 good-turn lengths |
| `decisions` | `id` | model, purpose, questions, answers, cost, latency, shadow, outcome label |
| `calibration` | `(model_version, purpose)` | thresholds, sample count, precision/recall |
| `events` | `id` | every action: kind, session, inputs, evidence ids, explanation template id + params |
| `ledger` | `session` | tokens by class, priced cost, tokenimum overhead |
| `installs` | `(host, agent_home)` | files written, entry ids, backup paths, shim hashes |

Migrations versioned in `tkm-store`. Table and column names are a compatibility surface once
released. Other files: `~/.local/share/tokenimum/errors.log` (rotated, 5 MB),
`$XDG_RUNTIME_DIR/tokenimum/tkmd.sock` (mode 600).

### 10.2 Layered configuration

Precedence, lowest to highest:

1. built-in defaults;
2. preset (`preset = "balanced"` in any layer);
3. global `~/.config/tokenimum/config.toml`;
4. repository `<repo>/.tokenimum.toml` (optional; may be committed);
5. repository local `<repo>/.tokenimum.local.toml` (added to `.git/info/exclude`);
6. environment `TKM_<SECTION>_<NAME>` (for example `TKM_LOOP_MODE=shadow`);
7. command-line flags of one `tkm` invocation.

`TKM_DISABLE=1` turns tokenimum off for one process tree: `tkm-hook` replies with the no-op shape
without contacting the daemon. `TKM_CONFIG` points at an alternative global file.

A repository file cannot raise budgets, change the API key path, enable `prune.mode = "apply"`, or
add `loop.escalate_command`; those keys are accepted only from global, env and flags, because a
cloned repository is untrusted input.

### 10.3 Presets

| Preset | Facts | Loop guard | Pruning | Decisions | Injection |
|---|---|---|---|---|---|
| `observe` | learn, inject off | shadow | report | on, shadow | off |
| `balanced` (default) | on | redirect after calibration; shadow before | propose | on | on |
| `aggressive` | on, `promote_confirmations = 2` | redirect, TE `aggressive` level | apply | on, agreement mode off | on, `max_tokens = 900` |
| `offline` | on, code evidence only | rules + backstop only | propose | off | facts only |

### 10.4 Per-mechanism switches

```toml
[facts]    enabled = true
[inject]   enabled = true      # also: max_tokens, memory, instructions_file
[loop]     mode = "redirect"   # off | shadow | redirect
[prune]    mode = "propose"    # off | report | propose | apply
[ledger]   enabled = true
[decide]   enabled = true      # chain, budgets, agreement
[hosts.claude]   enabled = true  # one table per harness; level = "auto" | "hooks" | "shim" | "fold-only"
[integrations]   superorchestrator = "auto"; otherskill = "auto"
[ui.web]   enabled = false; bind = "127.0.0.1:7878"
```

`tkm config` subcommands:

| Command | Does |
|---|---|
| `tkm config show [--effective]` | merged config; with `--effective`, each value's source layer |
| `tkm config get <key>` / `set <key> <value> [--global\|--repo\|--local]` / `unset` | edit one layer, validated against the schema |
| `tkm config preset <name> [--repo]` | switch preset |
| `tkm config explain <key>` | meaning, default, allowed values, current source |
| `tkm config edit [--global\|--repo]` | open in `$EDITOR`, validate on save |
| `tkm config diff` | difference from the preset |

## 11. Usability, monitoring and explanations

### 11.1 One-command install: `tkm init`

1. Detect harnesses: binaries on `PATH` (`claude`, `codex`, `agy`, `agent`/`cursor-agent`, `kimi`,
   `codewhale`/`deepseek`, `opencode`, `pi`) and agent homes on disk (`~/.claude`, `~/.codex`,
   `~/.gemini`, `~/.cursor`, `~/.kimi`, `~/.codewhale`, `~/.config/opencode`, `~/.pi/agent`).
2. Detect integrations (section 9.1).
3. Show a plan: per harness, integration level, files to change, trust steps; per adapter,
   enable or not. Nothing is written yet.
4. Ask once for the OpenRouter key (or reuse an existing key file by path, or choose `offline`).
5. Write hooks, generate shims (unless `--no-shims`), install the systemd user unit or launchd
   agent, start `tkmd`, run `tkm doctor`.
6. Print the remaining manual trust steps (Codex `/hooks`, Codewhale `/hooks approve`, Pi
   `/reload`).

Flags: `--yes` (non-interactive), `--dry-run`, `--only claude,codex`, `--home <dir> --host <h>`,
`--superorchestrator`, `--no-shims`, `--preset <name>`. `tkm uninstall` reverses exactly what the
`installs` table records.

### 11.2 `tkm status`

One screen, plain text, for the current repository (or `--all`):

- daemon state, version, uptime; adapters on/off and why;
- harnesses: level, last event seen, events missing per `tkm doctor`;
- facts: active, candidates, retired in the last 7 days;
- loop guard: mode, envelope, redirects and escalations in the last 7 days;
- pruning: mode, pending proposals;
- spend: decisions today vs daily cap; savings estimate with sample sizes;
- the last 5 actions, each with a one-line explanation and its event id.

`--json` emits the same data.

### 11.3 Live monitor: `tkm top`

A ratatui TUI over the monitor API. Panes (tab to switch, `?` for help):

| Pane | Shows |
|---|---|
| Sessions | active sessions: harness, repository, turn, tool calls this turn vs envelope, loop state |
| Decisions | live feed: purpose, model, answers, latency, cost, shadow or acted |
| Spend | decision spend today vs caps; per-model share; sparkline over 7 days |
| Savings | cost per session trend per repository and harness, with baseline and sample sizes |
| Facts | active facts with scores; candidates nearing promotion; recent retirements |
| Redirects | redirects and escalations with the triggering calls |

Enter on any row opens `tkm why` for it. Refresh is push-based (daemon stream), no polling. Works
read-only against the DB when the daemon is down (no live feed).

### 11.4 Optional web dashboard: `tkm dashboard`

- Feature `web` in `tkm-ui` (axum); off by default (`ui.web.enabled = false`).
- Binds `127.0.0.1` only; a random access token per daemon start, printed by `tkm dashboard`
  (opens the browser with the token in the URL fragment).
- Same panes as `tkm top`, plus history charts and fact/prune evidence pages.
- Read-only, except three actions behind a confirmation: forget a fact, restore a pruned tool,
  switch the repository preset.
- Static assets embedded in the binary; no CDN, no external requests.

### 11.5 Plain-language explanations: `tkm why <event>`

Every action stores its evidence ids and an explanation template. `tkm why <event-id>` (or
`tkm why last`, `tkm why --session <id>`) prints, for example:

```
Event 4812, 2026-11-03 14:22, Claude Code, repo github.com/acme/api

What happened: tokenimum added a short note to the agent's context:
  "Run tests with `python -m pytest` (`pytest` fails here: ImportError)."

Why: in 4 of the last 5 sessions in this repository, `pytest` failed with an
ImportError and `python -m pytest` then worked. The fact was confirmed in 3
sessions (needed: 3) and never contradicted.

Model checks: inception/mercury-decide-20260930 rated it stable (0.94) and safe (0.97).
Cost of those checks: $0.000006.

Undo: tkm facts forget f-17
```

Every action kind has a template: fact injected, fact promoted, fact retired, redirect, escalation,
deny-list proposal, deny-list write, restore, budget reached, adapter disabled. Templates are code,
never model output.

## 12. CLI reference

| Command | Does |
|---|---|
| `tkm init [flags]` | detect and install (section 11.1) |
| `tkm status [--all] [--json]` | section 11.2 |
| `tkm top` | live TUI monitor |
| `tkm dashboard` | start or open the local web dashboard |
| `tkm why <event\|last> [--session id]` | plain-language explanation of an action |
| `tkm facts [--all\|--brief]` / `tkm facts forget <id>` | facts with evidence; retire a fact |
| `tkm prune [apply [tool]\|restore [tool]]` | deny-list proposal, evidence, apply, give back |
| `tkm skills [--json]` | skill loaded vs used evidence |
| `tkm config ...` | section 10.4 |
| `tkm models [probe]` | decision chain, availability, calibration state |
| `tkm calibrate [--purpose loop\|facts\|relevance]` | fit thresholds from labelled decisions |
| `tkm replay <session-or-fixture>` | run mechanisms over a recorded session, print actions |
| `tkm run <harness> [args...]` | wrapper for fold-only harnesses |
| `tkm doctor [--probe]` | daemon, socket, DB, hooks per home, harness versions, events seen, shim hashes, key, model probes |
| `tkm export [--show-digests]` | JSON dump of one repository's state |
| `tkm uninstall [--host h]` | remove tokenimum's entries and shims |

A `tokenimum` skill for each harness wraps the read-only commands, so agents can call them; its
description must not claim effect.

## 13. Language and safety policy: entirely safe Rust

### 13.1 Own code

- Every crate root: `#![forbid(unsafe_code)]`. Workspace lint `unsafe_code = "forbid"`.
- CI fails if any crate lacks the attribute (a grep check plus `cargo build`).
- No `build.rs` that compiles C in tokenimum's own crates.
- Lints: `clippy::pedantic` warnings reviewed, `clippy::unwrap_used` and `clippy::expect_used`
  denied outside tests, `#![deny(missing_docs)]` on public library crates.
- Panics in `tkm-hook` are caught at the top level (`std::panic::catch_unwind` is safe Rust) and
  turned into the no-op reply.

### 13.2 Dependencies

- `cargo-deny` in CI: advisories (RustSec), licenses (allow Apache-2.0, MIT, BSD, ISC, Zlib,
  Unicode, MPL-2.0 file-level), bans (duplicate major versions reviewed), sources (crates.io only).
- `cargo-geiger` report in CI on every dependency change. Policy:
  - prefer crates that declare `#![forbid(unsafe_code)]`;
  - a dependency with `unsafe` is allowed only when listed in `docs/unsafe-allowlist.md` with a
    justification (for example: tokio, mio, rustls and its crypto provider, rusqlite and
    `libsqlite3-sys` bundling SQLite C code, crossterm for terminal I/O);
  - a new unjustified `unsafe` count in the geiger diff fails CI;
  - crates with `unsafe` but no maintenance in 12 months are rejected.
- `cargo audit` daily in CI. `Cargo.lock` committed.
- Static release builds (musl on Linux) through `cargo-dist` or equivalent; reproducible-build
  check in CI.

The honest limit: safe Rust for tokenimum's own code, audited and minimized `unsafe` in
dependencies. SQLite and the TLS crypto provider are the largest non-Rust parts (open question 3).

### 13.3 The TypeScript exception

opencode and Pi expose only TypeScript plugin APIs (section 6.11). The generated shims are the
only non-Rust code tokenimum ships. Rules: generated from a template at install time, ≤ 40 lines,
no dependencies, no logic, hash-checked by `tkm doctor`, removable with `tkm uninstall`, and
optional (`--no-shims` keeps those harnesses in fold-only mode). Open question 1.

### 13.4 Runtime safety and privacy

- Fail open on every path; errors go to `errors.log`.
- Never stop a turn, never cancel a run, never delete a skill.
- Never touch a settings entry tokenimum did not write; back up before the first write.
- Digests are redacted (8.4); `tkm export --show-digests` lets the person see exactly what is
  sent.
- Injected text comes only from templates and stored facts; a lint rejects facts that read as
  completion or verification advice (TE rule) and facts containing paths outside the repository.
- Facts derived from command output a repository controls can carry prompt injection. Fact
  parameters are restricted to command forms, env var names, host names and test labels, each
  validated by regex; free text from output is never injected.
- Repository config is untrusted (section 10.2).
- The socket and the dashboard are local only; the dashboard requires its token.

## 14. Performance budget

| Path | Budget | How |
|---|---|---|
| `tkm-hook` start to exit, `tool_done` | p50 < 3 ms, p99 < 10 ms | static binary, no runtime, one socket round trip |
| `tkmd` handling of `tool_done` | < 200 µs | in-memory turn state, no disk write on the hot path |
| `prompt` with relevance scoring | < 1,500 ms, cached reply < 5 ms | one decision request, cache |
| Shim overhead (opencode, Pi) | p50 < 15 ms, p99 < 40 ms | one process spawn per event; `tool_done` spawned without awaiting the reply when nothing is pending |
| Fold of one session | < 500 ms CPU, excluding decisions | streaming JSONL parse; read-only SQLite for agy and opencode |
| `tkmd` idle memory | < 30 MB (< 40 MB with web) | |
| `tkmd` idle CPU | ~0 | event-driven; file watching; 60 s timer only where watching is unavailable |
| `tkm top` refresh | < 50 ms per frame | push stream |

Benchmarks (criterion + hyperfine) run in CI against fixtures; a regression over 20% fails CI.

## 15. Milestones

| Milestone | Scope | Exit criteria |
|---|---|---|
| M0 | Decision-model bench: `tkm-decide` + `tkm replay` over 50 recorded Claude Code and Codex sessions from this machine; compare chain models on loop and fact questions | report of agreement, latency, cost per model; thresholds proposed |
| M1 | `tkm-hook`, `tkmd`, store, config layers, Claude Code host, loop guard in shadow mode; safe-Rust CI (forbid, cargo-deny, geiger) | p99 hook < 10 ms; shadow decisions logged on real sessions; CI policy green |
| M2 | Facts (observe, envfacts, filter, lifecycle, injection) + ledger, Claude Code + Codex | facts form on recorded fixtures as TE's do; ledger matches session cost within 2% |
| M3 | Usability: `tkm init` with detection, `tkm status`, `tkm top`, `tkm why`, presets, per-mechanism switches | fresh machine to working install in one command; every action explainable |
| M4 | Hook-level harnesses: agy, Cursor, Kimi Code, DeepSeek harness; idle-fold fallback | fold works on a real session of each; `tkm doctor --probe` reports seen events; to-verify items closed or documented |
| M5 | Shim-level harnesses: opencode, Pi; Rust-only fold-only mode for both | shim ≤ 40 lines, hash-checked; fold-only works with `--no-shims` |
| M6 | Context budget; tool pruning with protected set; otherskill adapter | duplicate-free injection under 600 tokens; no protected tool ever in a deny list (test); all four combinations of 9.2 pass integration tests |
| M7 | superorchestrator adapter: account-home install, run detection, events, spend report | delegated-run escalation reaches the orchestrator; spend visible in superorchestrator status |
| M8 | Calibration loop; leave shadow mode per purpose when precision targets are met; web dashboard | precision ≥ 0.9 for `stuck` on labelled turns; dashboard local-only with token |

## 16. Testing

- Unit: `tkm-core` pure functions, table-driven, ported TE tests where they exist
  (`tests/test_ledger.py`, `test_envfacts.py`, `test_injection.py` as behavioural references).
- Golden: replay fixtures per harness (all eight); expected actions committed as JSON.
- Host adapter contract tests: for each harness, recorded hook payloads in, expected reply bytes
  out; reply shapes validated against the shapes in section 6.3.
- Installer tests: each settings writer runs against copies of real config files with foreign
  entries (otherskill, rtk); foreign entries survive byte-for-byte; uninstall restores the
  original.
- Parity: for Claude Code and Codex fixtures, tokenimum's fact formation and loop gate agree with
  TE run over the same logs (TE is the oracle in M2; differences documented).
- Integration matrix: the four combinations of section 9.2, using fake `superorchestrator` and
  `otherskill` binaries on `PATH`.
- Decision tests use recorded responses (no network); one opt-in live test per model behind
  `TKM_LIVE=1`.
- Fault injection: daemon down, socket slow, OpenRouter 404/500/timeout, DB locked, corrupt log,
  foreign SQLite locked, shim timeout. Each must yield the no-op reply and an error line, never a
  hook failure.
- Safety: CI check for `#![forbid(unsafe_code)]` in every crate; `cargo-deny`; `cargo-geiger` diff.

## 17. Open questions

1. opencode and Pi require TypeScript plugins. Is a generated ≤ 40-line shim an acceptable
   exception to the Rust-only rule, or should both stay fold-only by default? (Sections 6.11, 13.3.)
2. Which harness is "the DeepSeek harness"? The spec assumes Codewhale (formerly DeepSeek-TUI,
   community, not affiliated with DeepSeek). (Section 6.10.)
3. SQLite (C, via `libsqlite3-sys`) and the TLS crypto provider are the largest non-Rust,
   `unsafe`-bearing dependencies. Accept with the allowlist, or evaluate a pure-Rust store?
4. otherskill hand-off: should otherskill call tokenimum for relevance scoring, or tokenimum call
   otherskill (current spec)? One injector or two? (Section 9.4.)
5. superorchestrator API for events and spend (CLI, MCP, or its socket); and whether `tkmd`
   should run inside the superorchestrator supervisor when that adapter is enabled.
6. Port TE code (Apache-2.0, keep attribution and NOTICE) or re-implement from the paper?
   Default: port logic, re-implement I/O.
7. Agreement mode on by default for the loop guard while calibration data is thin?
8. Cursor CLI hook coverage and agy, Kimi, Codex `exec` behaviours: close the "to verify" items in
   section 6 during M4.
9. Should the instructions-file injection (fold-only mode) be allowed for committed `AGENTS.md`
   files, or only for untracked ones?

## 18. To-verify register

| Harness | Item | Link |
|---|---|---|
| Claude Code | ~~project-settings hook trust rule; Stop/SessionEnd payload fields~~ closed 2026-10-29, `docs/verified/claude-codex.md` | https://code.claude.com/docs/en/hooks |
| Claude Code | ~~permissions deny syntax~~ closed 2026-10-29 | https://code.claude.com/docs/en/permissions |
| Codex | ~~hooks under `codex exec`~~ closed 2026-10-29 (trusted hooks only) | https://developers.openai.com/codex/hooks |
| Codex | ~~tool deny key in `config.toml` / `rules/`; MCP tool filters~~ closed 2026-10-29: no built-in deny key, report only | https://developers.openai.com/codex/config-reference |
| agy | ~~exit-code semantics; no-op reply; project-hook trust; notice field; permissions deny syntax; `transcriptPath` format; DB blob schema~~ closed 2026-10-29, `docs/verified/e5b-hosts.md`; still open: `PreToolUse`/`PostToolUse` under `agy -p` (not observed) | https://antigravity.google/docs/hooks?tab=cli |
| Cursor | ~~which hooks fire in the CLI; default timeout; `sessionStart` injection; third-party hook loading~~ closed 2026-10-29 (CLI facts from forum); transcript format only forum-reported | https://cursor.com/docs/hooks |
| Kimi Code | ~~project-level hooks; plugin hooks; stdout injection per event~~ closed 2026-10-29 (stdout not consumed in 1.52.0) | https://raw.githubusercontent.com/MoonshotAI/kimi-cli/main/docs/en/customization/hooks.md |
| DeepSeek harness | ~~which harness is meant; session file format~~ closed 2026-10-29 (Codewhale; `sessions/<uuid>.json`) | https://github.com/Hmbown/DeepSeek-TUI/blob/main/docs/HOOKS.md |
| opencode | ~~plugin hook timeouts; toast API; `session.created` under `opencode run`; MCP permission keys~~ closed 2026-10-29, `docs/verified/opencode-pi.md` (`session.created` from source, not run) | https://opencode.ai/docs/plugins/, https://opencode.ai/docs/permissions/ |
| Pi | ~~`ctx.ui` notify API; handler timeouts~~ closed 2026-10-29; async work in `session_shutdown` stays open (not in the docs, needs a runtime probe), `docs/verified/opencode-pi.md` | https://github.com/badlogic/pi-mono/blob/main/packages/coding-agent/docs/extensions.md |
| Gemini CLI | whether its hooks apply to agy (assumed not) | https://geminicli.com/docs/hooks/ |

## 19. References

- microsoft/TokenEconomizer: README, `docs/PLUGIN.md`, `docs/ARCHITECTURE.md`,
  `tokeneconomizer/{microskills,toolprune,loopguard,plugin}` — https://github.com/microsoft/TokenEconomizer
- TE paper: https://arxiv.org/abs/2610.22732
- OpenRouter Decisions API reference:
  https://openrouter.ai/docs/api/api-reference/alphadecisions/submit-a-decisions-questions-and-answers-request.md
- Jev guide: https://openrouter.ai/docs/guides/community/jev
- Tool gating with Jev: https://openrouter.ai/docs/cookbook/building-agents/gate-tool-calls-with-jev
- Decision model list: https://openrouter.ai/api/v1/models?output_modalities=decisions
- Claude Code hooks: https://code.claude.com/docs/en/hooks
- Codex hooks: https://developers.openai.com/codex/hooks
- Antigravity CLI hooks: https://antigravity.google/docs/hooks?tab=cli; plugins: https://antigravity.google/docs/cli/plugins
- Cursor hooks: https://cursor.com/docs/hooks; CLI permissions: https://cursor.com/docs/cli/reference/permissions
- Kimi Code hooks and data locations: https://github.com/MoonshotAI/kimi-cli/tree/main/docs/en
- Codewhale (DeepSeek-TUI) hooks: https://github.com/Hmbown/DeepSeek-TUI/blob/main/docs/HOOKS.md
- opencode plugins and permissions: https://opencode.ai/docs/plugins/, https://opencode.ai/docs/permissions/;
  types: https://cdn.jsdelivr.net/npm/@opencode-ai/plugin/dist/index.d.ts
- Pi extensions, session format, settings: https://github.com/badlogic/pi-mono/tree/main/packages/coding-agent/docs
- cargo-deny: https://github.com/EmbarkStudios/cargo-deny; cargo-geiger: https://github.com/geiger-rs/cargo-geiger
