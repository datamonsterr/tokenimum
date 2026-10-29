# opencode and Pi: to-verify items (SPEC 18)

Checked 2026-10-29 against the official docs, the opencode source at tag v1.18.35 (the locally
installed version; repository `anomalyco/opencode`, formerly `sst/opencode`) and the published
`@opencode-ai/plugin` / `@opencode-ai/sdk` 1.18.35 type definitions. Pi is not installed locally:
its items rest on the extension docs only and are marked as such. Nothing under
`~/.config/opencode`, `~/.local/share/opencode` or `~/.pi` was modified; the SQL fixture is a
redacted subset of a local database copied to a scratch directory.

## opencode

| Item | Result | Evidence |
|---|---|---|
| Plugin hook timeouts | **Closed.** None. `Plugin.trigger` awaits each plugin's hook in turn with no timer, so a slow `chat.message` or `tool.execute.after` handler blocks the prompt or tool result. The `event` hook is called as `void hook["event"]?.(...)` and is not awaited. The shim therefore races its own spawn against a timer (3,000 ms for `prompt`, 1,000 ms otherwise). | [plugin/index.ts L255-300](https://github.com/anomalyco/opencode/blob/v1.18.35/packages/opencode/src/plugin/index.ts); docs: [plugins](https://opencode.ai/docs/plugins/) (no timeout is documented) |
| Toast API | **Closed.** `client.tui.showToast({ body: { title?, message, variant: "info" \| "success" \| "warning" \| "error", duration? } })` returns `boolean`. Without a TUI (for example `opencode run`) the call has no UI to reach; the shim ignores any error. | `@opencode-ai/sdk` 1.18.35 `dist/gen/types.gen.d.ts` `TuiShowToastData`, `dist/gen/sdk.gen.d.ts` `showToast`; [plugins, TUI events](https://opencode.ai/docs/plugins/) |
| `session.created` under `opencode run` | **Closed from source, not probed at runtime.** `opencode run` creates its session with `sdk.session.create`, which publishes `Session.Event.Created`; plugins receive bus events whose location matches their directory. So the event is expected under `run` (without `--attach`, where the server owns the plugins). Because this was not run, the daemon keeps the safe fallback: it creates a session on the first event it sees for an unknown id, and the idle-fold (`session.idle` plus the read-only `opencode.db` fold) covers a missing start. | [cli/cmd/run.ts L519, L554](https://github.com/anomalyco/opencode/blob/v1.18.35/packages/opencode/src/cli/cmd/run.ts), [session/session.ts L535](https://github.com/anomalyco/opencode/blob/v1.18.35/packages/opencode/src/session/session.ts), [plugin/index.ts L255-259](https://github.com/anomalyco/opencode/blob/v1.18.35/packages/opencode/src/plugin/index.ts) |
| MCP permission keys | **Closed (naming); wildcard use inferred.** Permission keys are tool names. An MCP tool is registered as `<server>_<tool>` with both parts sanitized by replacing every character outside `[a-zA-Z0-9_-]` with `_`. So `"<server>_<tool>": "deny"` denies one tool. The permissions page documents `*` wildcards (top-level `"*"` and in patterns); `"<server>_*"` is expected to work the same way but was not run. tokenimum writes exact keys and uses the wildcard only if CI/E7 probes confirm it. | [mcp/catalog.ts L117-119](https://github.com/anomalyco/opencode/blob/v1.18.35/packages/opencode/src/mcp/catalog.ts) (`toolName`), [permissions](https://opencode.ai/docs/permissions/) |

Further facts the adapter depends on (same sources, same date):

- Plugin file locations and auto-load: `~/.config/opencode/plugins/` and `<repo>/.opencode/plugins/`
  ([plugins](https://opencode.ai/docs/plugins/)). The local plugins `otherskill.ts` and `rtk.ts`
  use the same export style.
- `chat.message` runs with `(input: {sessionID, agent?, model?, messageID?, variant?}, output:
  {message: UserMessage, parts: Part[]})`; parts pushed into `output.parts` are persisted with the
  message (`session/prompt.ts` triggers the hook, then saves `resolvedParts`). Injected parts carry
  `synthetic: true` and must have an id starting with `prt`: `PartID` checks `isStartsWith("prt")`
  ([session/schema.ts L19](https://github.com/anomalyco/opencode/blob/v1.18.35/packages/opencode/src/session/schema.ts));
  prompt.ts L1033-1048 logs a part that fails decoding and persists the parts. The shim uses
  `prt_tkm_<message id>`.
- `tool.execute.after` runs with `(input: {tool, sessionID, callID, args}, output: {title, output,
  metadata})`; `metadata.exit` is present for `bash`, `metadata.exists` for `write`. It carries
  no duration or working directory (the shim runs `tkm-hook` in the plugin `directory`).
- `event` receives `{ id, type, properties }`. `session.created` and `session.deleted` carry
  `properties.info` (a `Session` with `id`, `directory`, `time.created`); `session.idle` carries
  `properties.sessionID`.
- Bun `$` accepts a `Response` as a stdin redirect (`< ${response}`), `.cwd()`, `.quiet()`,
  `.nothrow()` and `.text()` (`shell.d.ts`). The shim was exercised under Bun with a fake
  `tkm-hook`: reply applied, 1,000 ms timeout enforced, garbage output ignored.
- Local database v1.18.35 schema: tables `project`, `session`, `message`, `part`; message and part
  `data` are JSON. User prompt text lives in `text` parts, not on the message row. Assistant
  `tokens` is `{total, input, output, reasoning, cache: {read, write}}`; `input` excludes cache
  (verified: `total = input + output + reasoning + cache.read`). `time_*` columns are Unix
  milliseconds.
- Permission surface for pruning: `opencode.json` `permission` object, `"<tool>": "deny"` or a
  pattern object ([permissions](https://opencode.ai/docs/permissions/)). Rules are evaluated by
  pattern, last match wins; `opencode.json` in the repository root is the project scope.

## Pi

| Item | Result | Evidence |
|---|---|---|
| `ctx.ui` notify API | **Closed.** `ctx.ui.notify(message, type?)` with `type` `"info" \| "warning" \| "error"`; fire-and-forget, works in TUI and RPC modes and is a no-op in print and JSON modes (`ctx.hasUI` is `false` there). | [extensions.md, ctx.hasUI and Custom UI dialogs](https://github.com/earendil-works/pi/blob/main/packages/coding-agent/docs/extensions.md) |
| Handler timeouts | **Closed as "none documented".** The docs describe no timeout for event handlers; "Extension errors are logged, agent continues". Only `pi.exec(command, args, {signal, timeout, cwd, maxBuffer})` takes a timeout (it terminates the process, SIGKILL after 5 s if SIGTERM is ignored). The shim passes 3,000 ms for `before_agent_start` and 1,000 ms elsewhere and awaits the result, so a handler blocks Pi for at most that long. | same page, Error Handling and `pi.exec` |
| Async work in `session_shutdown` | **Not determinable from the docs.** `session_shutdown` ("fired before a started session runtime is torn down", reason `quit`, `reload`, `new`, `resume`, `fork`) does not say whether handlers are awaited or bounded on exit (Ctrl+C, Ctrl+D, SIGHUP, SIGTERM). The shim awaits one `pi.exec` with a 1,000 ms timeout and treats `session_end` as best effort: the daemon also ends a session on idle timeout and through the fold. Needs a runtime probe when Pi is available. | same page, Session Events and lifecycle diagram |

Further facts the adapter depends on (same sources, same date):

- Extensions load from `~/.pi/agent/extensions/*.ts` (global) and `.pi/extensions/` (project, needs
  project trust); `/reload` loads a new file.
- `pi.on("before_agent_start", ...)` may return `{ message: { customType, content, display } }`;
  the message is stored in the session and sent to the model. `event.messageType` is `"user"` (with
  `prompt`) or `"custom"` (no prompt).
- `agent_end` fires once per run, but Pi may still retry or continue with queued messages;
  `turn_end` fires per model response. `agent_settled` is the final-idle signal (not used).
- `tool_result` event: `toolName`, `toolCallId`, `input`, `content`, `details`, `isError`,
  `usage`. `ctx.cwd` is the working directory; `ctx.sessionManager.getSessionId()` the session id.
- Session JSONL v3 and message shapes: [session-format.md](https://github.com/earendil-works/pi/blob/main/packages/coding-agent/docs/session-format.md),
  [message-types.md](https://github.com/earendil-works/pi/blob/main/packages/coding-agent/docs/message-types.md)
  (`toolCall` blocks `{id, name, arguments}`, usage `{input, output, cacheRead, cacheWrite,
  reasoning?}` with `reasoning` already inside `output`, `stopReason`). The reader test uses a
  synthetic file built from these pages; no real Pi session was available.
- Pruning surface: `defaultTools` in `<repo>/.pi/settings.json` with `-name` entries
  ([settings.md](https://github.com/earendil-works/pi/blob/main/packages/coding-agent/docs/settings.md));
  `/reload` applies newly added tools but does not disable removed ones until restart.
