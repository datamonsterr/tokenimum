# Agent skills for tokenimum development

Canonical location: `.agents/skills/` (read by Codex, agy, opencode and other harnesses that use
the shared `.agents` convention). `.claude/skills` is a symlink to this directory for Claude Code.
Pi, Kimi Code, Cursor and the DeepSeek harness use their own skill directories; point them here or
copy as needed.

Third-party skills were found with otherskill (local bank plus online search) and GitHub search,
read in full before copying, and copied unchanged with their license. Their bundled scripts were
not run. Treat them as reference material, not as trusted code.

## Third-party skills

| Skill | Source (commit) | License | Why it is here |
|---|---|---|---|
| `unsafe-checker` | https://github.com/actionbook/rust-skills/tree/5c40d3ad785193231b7d0dbfb8e1eb447e5edd94/skills/unsafe-checker | MIT, declared in upstream README; no LICENSE file upstream (see `LICENSE-NOTE.md`) | Audit `unsafe` in dependencies and keep tokenimum's own crates free of it (SPEC 13). |
| `m06-error-handling` | https://github.com/actionbook/rust-skills/tree/5c40d3ad785193231b7d0dbfb8e1eb447e5edd94/skills/m06-error-handling | same as above | Fail-open error design; no `unwrap` on untrusted input. |
| `m07-concurrency` | https://github.com/actionbook/rust-skills/tree/5c40d3ad785193231b7d0dbfb8e1eb447e5edd94/skills/m07-concurrency | same as above | `tkmd` concurrency: tokio tasks, channels, Send/Sync. |
| `domain-cli` | https://github.com/actionbook/rust-skills/tree/5c40d3ad785193231b7d0dbfb8e1eb447e5edd94/skills/domain-cli | same as above | `tkm` CLI design with clap, config files, env vars, TUI. |
| `rust-async-patterns` | https://github.com/wshobson/agents/tree/0874a27b17a22e2b7c2e2bcd725768e0803cad06/plugins/systems-programming/skills/rust-async-patterns | MIT | Tokio patterns for the daemon, socket server and decision client. |
| `rust-best-practices` | https://github.com/apollographql/skills/tree/ff4534868e089580c2e09563c20f86a410179628/skills/rust-best-practices | MIT | Idiomatic Rust, testing and documentation conventions. |
| `ratatui` | https://github.com/FlorianBruniaux/ccboard/tree/c1a36a0ed5296279ce85b0009a3aa63ea62e78b8/.claude/skills/ratatui | MIT | `tkm top` monitor (SPEC 11.3). |
| `rusqlite` | https://github.com/claude-dev-suite/claude-dev-suite/tree/e10ab644ba43abe2abc923399226df4af7337214/skills/databases/rusqlite | MIT | `tkm-store`: WAL, migrations, read-only access to foreign SQLite logs. |
| `hook-development` | https://github.com/anthropics/claude-plugins-official/tree/f8c5e7a1dcb71b10cb74586224b5401f3de5db28/plugins/plugin-dev/skills/hook-development | Apache-2.0 | Claude Code hook events, payloads and testing; the model other harnesses copy. |

## Skills written for this repository

| Skill | Why |
|---|---|
| `release-management` | Version bumps, release PRs, tags, artifact verification and rollback. |
| `tkm-harness-hooks` | Per-harness hook config, events, reply shapes, log locations and doc links for all eight harnesses, plus the verify-before-code checklist. No suitable licensed third-party skill covers Cursor, Kimi Code, Codewhale, opencode or Pi hooks. |
| `openrouter-decisions` | Rules for the Decisions API client. OpenRouter's own `openrouter-decisions` skill (https://github.com/OpenRouterTeam/skills) has no license file, so it is linked, not copied. |

## Considered and not included

- `trailofbits/skills` `rust-review` (CC-BY-SA-4.0): strong Rust security review, but it depends on
  plugin subagents and Python scripts. Install it as a plugin when a review is needed.
- `OpenRouterTeam/skills`: no license file; linked from `openrouter-decisions`.
- opencode, Cursor and Pi hook skills found by search: none had a clear license and official
  content; covered by `tkm-harness-hooks` with links to official docs instead.
- Criterion, cargo-deny and cargo-geiger skills: no good match; SPEC 13.2 and 14 hold the policy.
