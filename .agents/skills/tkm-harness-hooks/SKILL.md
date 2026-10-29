---
name: tkm-harness-hooks
description: Use when writing or changing a tokenimum host adapter, hook installer, shim, or session log reader for Claude Code, Codex, Antigravity (agy), Cursor, Kimi Code, the DeepSeek harness (Codewhale), opencode, or Pi. Gives the hook config paths, event names, reply shapes, log locations and official doc links, and the rules for verifying them.
---

# tokenimum harness hooks

SPEC.md section 6 is the source of truth. This skill is the working checklist.

## Rules

1. Never code a hook API from memory. Open the official doc link below first and compare it with
   SPEC.md section 6. If they differ, update SPEC.md in the same change.
2. Items marked "to verify" in SPEC.md section 18 must be confirmed (doc or local probe) before
   code depends on them. Record the evidence (doc URL plus date, or the probe command and output)
   in the commit message.
3. Record a real hook payload for every new event: run the harness with a hook that writes stdin
   to a file in a scratch directory, redact it, and add it under `fixtures/<host>/`.
4. tokenimum replies never block, never stop a turn, and never rewrite a prompt.
5. Installers add only tokenimum entries, keep foreign entries byte-for-byte, back up before the
   first write, and are idempotent.
6. Rust only. opencode and Pi are the one exception: the generated shim in `shims/` stays at or
   under 40 lines with no logic and no dependencies.

## Quick reference

| Harness | Hook config | Events (start / prompt / tool / stop / end) | Inject | Docs |
|---|---|---|---|---|
| Claude Code | `~/.claude/settings.json` `hooks` | SessionStart / UserPromptSubmit / PostToolUse / Stop / SessionEnd | `hookSpecificOutput.additionalContext` | https://code.claude.com/docs/en/hooks |
| Codex | `~/.codex/hooks.json` (trust via `/hooks`) | same names as Claude Code | same shape; cap via `additionalContextLimit` | https://developers.openai.com/codex/hooks |
| agy | `~/.gemini/config/hooks.json`, top-level name `tokenimum` | SessionStart / PreInvocation / PostToolUse / Stop / none | `injectSteps` | https://antigravity.google/docs/hooks?tab=cli |
| Cursor | `~/.cursor/hooks.json` (`version: 1`) | sessionStart / beforeSubmitPrompt (no inject) / postToolUse / stop / sessionEnd | `additional_context` | https://cursor.com/docs/hooks |
| Kimi Code | `~/.kimi/config.toml` `[[hooks]]` | SessionStart / UserPromptSubmit / PostToolUse / Stop / SessionEnd | stdout on exit 0 | https://github.com/MoonshotAI/kimi-cli/blob/main/docs/en/customization/hooks.md |
| DeepSeek harness (Codewhale) | `~/.codewhale/config.toml` `[[hooks.hooks]]` | session_start / message_submit / tool_call_after / turn_end / session_end | `tool_call_before` `additionalContext` only | https://github.com/Hmbown/DeepSeek-TUI/blob/main/docs/HOOKS.md |
| opencode | shim `~/.config/opencode/plugins/tokenimum.ts` | session.created / chat.message / tool.execute.after / session.idle / session.deleted | text part or `output.system` | https://opencode.ai/docs/plugins/ |
| Pi | shim `~/.pi/agent/extensions/tokenimum.ts` | session_start / before_agent_start / tool_result / turn_end / session_shutdown | returned `message` | https://github.com/badlogic/pi-mono/blob/main/packages/coding-agent/docs/extensions.md |

## Session logs

| Harness | Location |
|---|---|
| Claude Code | `~/.claude/projects/<cwd-slug>/<session>.jsonl` |
| Codex | `$CODEX_HOME/sessions/YYYY/MM/DD/rollout-*.jsonl` (older `.jsonl.zst`) |
| agy | hook `transcriptPath`: `~/.gemini/antigravity-cli/brain/<id>/.system_generated/logs/transcript_full.jsonl` |
| Cursor | hook `transcript_path` (JSONL, forum-reported shape) |
| Kimi Code | `~/.kimi/sessions/<md5(workdir)>/<session-id>/` |
| Codewhale | `$CODEWHALE_HOME/sessions/<uuid>.json` |
| opencode | `~/.local/share/opencode/opencode.db` (open read-only) |
| Pi | `~/.pi/agent/sessions/--<cwd>--/<ts>_<id>.jsonl` |

## Checklist for a new or changed adapter

- [ ] Doc links re-read today; SPEC.md section 6 matches.
- [ ] Payload fixture recorded and redacted.
- [ ] Reply contract test: payload in, exact bytes out.
- [ ] Installer test against a config with foreign entries.
- [ ] `tkm doctor --probe` reports the events this harness delivers.
- [ ] Fold-only fallback still works with hooks disabled.
