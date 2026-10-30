# Doctor capability probes (2026-10-29)

`tkm doctor --probe <host>` installs the existing adapter in a fresh temporary HOME and working
directory, with a Rust recorder (`tkm` entered through a `tkm-hook` symlink). The recorder stores
native event names only. Credentials available through the environment remain available to the
host; saved credentials and the person's host configuration are not copied. Authentication failure
or a 30-second deadline failure produces fold-only fallback. Partial coverage uses idle-fold per
native event. A later probe replaces each event's result. Scratch directories are removed.

| Host | Probe command | Documentation / limitation |
|---|---|---|
| Claude | `claude -p <probe>` | [Print mode](https://code.claude.com/docs/en/headless), [hooks](https://code.claude.com/docs/en/hooks) |
| Codex | Recorded live events | [Hook trust](https://developers.openai.com/codex/hooks#review-and-trust-hooks): a fresh HOME has no trusted hook hashes. We do not bypass the person's hook trust. `codex exec` exists, but would incorrectly report every event missing in this HOME. |
| agy | `agy -p <probe>` | [Headless mode](https://antigravity.google/docs/cli/headless/), [CLI hooks](https://antigravity.google/docs/hooks?tab=cli). PostToolUse is previously unobserved and stays idle-fold unless this probe records it. |
| Cursor | `agent -p <probe>` (or `cursor-agent`) | [Headless CLI](https://cursor.com/docs/cli/headless), [hooks](https://cursor.com/docs/hooks). Prompt/stop delivery varies by version; each absence is recorded, not guessed. |
| Kimi | `kimi --print --prompt <probe>` | [CLI reference](https://raw.githubusercontent.com/MoonshotAI/kimi-cli/main/docs/en/reference/kimi-command.md), [hooks](https://raw.githubusercontent.com/MoonshotAI/kimi-cli/main/docs/en/customization/hooks.md). Injection remains subject to the documented caveat in e5b-hosts.md. |
| Codewhale | Recorded live events | `codewhale exec --hooks` delivers only tool_call_before/shell_env, not lifecycle hooks (existing source verification in [e5b-hosts.md](e5b-hosts.md)). The old upstream HOOKS.md URL is no longer fetchable on this date; no new API assumption is introduced. |
| opencode | `opencode run <probe>` | [CLI](https://opencode.ai/docs/cli/), [plugins](https://opencode.ai/docs/plugins/) |
| Pi | `pi --print <probe>` | [CLI](https://raw.githubusercontent.com/earendil-works/pi/main/packages/coding-agent/docs/cli.md), [extensions](https://raw.githubusercontent.com/earendil-works/pi/main/packages/coding-agent/docs/extensions.md). Uses agent_end, matching the existing shim. |

The probe asks the host to read a harmless scratch file, then stop. It does not force a failing
tool call: PostToolUseFailure is independently absent until observed. Missing lifecycle events are
handled by the existing log activity watcher and idle fold. For missing tool events, calls are
read from the transcript during that fold. A late event marked idle-fold does not trigger live
loop actions or premature session termination.

Tests: `crates/tkm-cli/tests/host_probe.rs` runs fake hosts through the Rust recorder, checks HOME
isolation and cleanup, partial delivery, missing executable and the Codewhale exception.
`crates/tkmd/tests/event_probe.rs` checks native-event independence and idle-fold dispatch.
