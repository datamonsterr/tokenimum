# ADR 0026: Fold-only mode library (E6.4)

Date: 2026-10-29. Status: accepted.

## Context

E6.4 delivers the fold-only library (`fold_only` module in `tkm-hosts`) that backs `--no-shims`
mode (ADR 0001 item 1) and provides the injection layer for harnesses without native hooks. It
covers four concerns:

1. **Instructions-file injection** – writing fact blocks into `AGENTS.md` / `CLAUDE.md`.
2. **Skill installation** – placing a `tokenimum/SKILL.md` in the harness skill directory.
3. **Run wrapper** – a pure library API for spawning a harness with session-tracking env vars.
4. **Mode selection** – mapping each host to its best integration mode.

## Decisions

### 1. Instructions file per harness

Verified from harness documentation:

| Harness | Instructions file | Evidence |
|---|---|---|
| Claude Code | `CLAUDE.md` | https://code.claude.com/docs/en/readme#project-memory (project memory) |
| Codex | `AGENTS.md` | https://developers.openai.com/codex/agents-md |
| agy | `AGENTS.md` | https://antigravity.google/docs/context#agents-md |
| Cursor | `AGENTS.md` | https://cursor.com/docs/context/agents-md |
| Kimi Code | `AGENTS.md` | https://github.com/MoonshotAI/kimi-cli/blob/main/docs/en/customization/agents-md.md |
| Codewhale (DeepSeek) | `AGENTS.md` | https://github.com/Hmbown/DeepSeek-TUI/blob/main/docs/AGENTS.md |
| opencode | `AGENTS.md` | https://opencode.ai/docs/context/agents-md |
| Pi | `AGENTS.md` | https://github.com/badlogic/pi-mono/blob/main/packages/coding-agent/docs/agents-md.md |

All harnesses except Claude Code read `AGENTS.md`. The `instructions_file(host)` function returns
`"CLAUDE.md"` for Claude and `"AGENTS.md"` for all others.

### 2. Markers

```
<!-- tokenimum:begin -->
...generated block...
<!-- tokenimum:end -->
```

The block carries a generated-by comment on its first interior line. `splice` replaces the marked
region in-place or appends with a blank-line separator (idempotent on same body). `remove` deletes
only the block and trailing blank line; byte positions outside markers are unchanged.

### 3. Creation marker (ADR 0011 §backup)

When tokenimum creates an instructions file from scratch, it writes an empty `.tkm-bak` sibling
(matching the convention in ADR 0011). `remove_from` deletes the file only when the `.tkm-bak`
is empty (file was created by tokenimum) and no content outside the markers remains.

### 4. git tracked-file refusal (SPEC 6.12, ADR 0001 item 9)

`apply` refuses to write to a file tracked by git unless `allow_tracked` is true (corresponds to
config key `inject.instructions_file_tracked`). The check reuses `writer::deny::refuse_tracked`
(via `git ls-files --error-unmatch`).

### 5. `.git/info/exclude` append

When tokenimum creates the instructions file, it appends the file's repo-relative path to the
file returned by `git rev-parse --git-path info/exclude`, exactly once. This mirrors the deny
writer in `writer/deny.rs`.

### 6. Skill directories per harness

Verified from documentation:

| Harness | Skill directory | Evidence |
|---|---|---|
| Claude Code | `<agent_home>/skills/tokenimum/SKILL.md` | https://code.claude.com/docs/en/skills |
| opencode | `~/.config/opencode/skills/tokenimum/SKILL.md` | https://opencode.ai/docs/skills/ |
| Pi | `~/.pi/agent/skills/tokenimum/SKILL.md` | https://github.com/badlogic/pi-mono/blob/main/packages/coding-agent/docs/skills.md |
| Codex | None documented | No official skill directory in Codex docs |
| agy | None documented | agy uses SKILL.md in `.agents/skills/` relative to repo; no user-level skill dir documented |
| Cursor | None documented | No skill directory in Cursor docs |
| Kimi Code | None documented | No skill directory in Kimi docs |
| Codewhale | None documented | No skill directory in Codewhale docs |

`skill_path` returns `None` for harnesses without a documented skill directory.

### 7. Skill content

The generated SKILL.md lists **only** read-only commands (`tkm facts --brief`,
`tkm why <event>`). It makes no claims about saving tokens or effects, as such claims would be
inaccurate (fold-only mode; the agent itself decides what to do with facts).

### 8. Run wrapper

`RunPlan` is pure data; `plan()` injects `TKM_SESSION_ID` and `TKM_HOST` environment variables.
`execute()` uses `std::process::Command` (synchronous, no async dependency), calls
`observer.started()` before `wait()` and `observer.ended()` after. A spawn failure skips
`ended()`. Time is supplied by the caller via a `Clock` trait to avoid hidden syscall dependencies
in tests.

### 9. Mode selection

| Host | `shims = true` | `shims = false` |
|---|---|---|
| Claude, Codex, agy, Cursor, Kimi, Codewhale | `Hooks` | `Hooks` |
| opencode | `Shim` | `FoldOnly` |
| Pi | `Shim` | `FoldOnly` |

Hooks-capable harnesses always use `Hooks` regardless of the `--no-shims` flag.
