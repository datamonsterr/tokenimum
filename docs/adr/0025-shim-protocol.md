# ADR 0025: Shim protocol for opencode and Pi (E6)

Date: 2026-10-29. Status: accepted (epic E6). Extends ADR 0011 (hook command line, ownership).
Verified host facts: `docs/verified/opencode-pi.md`.

## Context

opencode plugins and Pi extensions are in-process TypeScript (SPEC 6.11). tokenimum ships one
generated shim per host (at most 40 lines, no logic) that forwards events to `tkm-hook` and applies
its reply. This ADR fixes the wire contract between shim, `tkm-hook` (epic E4) and the adapters
(`tkm_hosts::opencode`, `tkm_hosts::pi`), the install rules, and the reader choices.

## Decisions

1. **Command line.** `tkm-hook --host <opencode|pi> --event <EventKind::as_str> --host-event
   <native name>`. opencode passes the payload on stdin. Pi's `pi.exec(cmd, args, {timeout})` has no
   stdin option, so Pi passes it as one argument `--payload <json>`. The Pi shim truncates every
   string longer than 8,000 characters (a `JSON.stringify` replacer) and fails open when the
   UTF-8 payload exceeds 100,000 bytes (Linux limits one argument to 128 KiB). E4 implements
   `--host-event` and `--payload`; E6 does not edit `crates/tkm-hook`.
2. **Payloads.**
   - opencode: `{"hook":"<hook>","input":..,"output":..}` for `chat.message` and
     `tool.execute.after` (the `output` of `chat.message` is `{message, parts}`, sent as is);
     `{"hook":"event","event":<event>}` for the three forwarded bus events (`session.created`,
     `session.idle`, `session.deleted`; the shim returns at once for other types).
     `--host-event` is the hook name, or the event `type` for `event` hooks. The adapter accepts a
     `host_event` equal to either and rejects any other value as a conflict.
   - Pi: `{"hook":"<event name>","event":<event>,"cwd":ctx.cwd,"session_id":ctx.sessionManager.getSessionId()}`.
     Pi events carry no session id, so the shim adds it (deviation from the first draft, which sent
     only `hook` and `event`). The event is sent without `messages`, `systemPrompt` and
     `systemPromptOptions` (large, may hold context files).
3. **Reply** (`render_reply`, printed verbatim by `tkm-hook`): compact JSON with keys in the order
   `context`, `notice`, each optional: `{"context":"..","notice":".."}`. `ReplyAction::Nothing`,
   an empty text and any reply with nothing to deliver render as the empty string
   (`Host::noop_reply()` is `""` for both hosts). Text is capped at 10,000 characters on a
   character boundary. Injection is allowed only on opencode `chat.message` and Pi
   `before_agent_start`; a notice on every event except session end. Parts an event does not allow
   are dropped. A request that fails mapping validation renders `""`.
4. **Applying the reply.** opencode `chat.message`: push `{id:"prt_tkm_"+output.message.id, sessionID,
   messageID, type:"text", text:context, synthetic:true}` into `output.parts`. The id must start with
   `prt`: `PartID` is `Schema.String.check(Schema.isStartsWith("prt"))`
   ([session/schema.ts L19](https://github.com/anomalyco/opencode/blob/v1.18.35/packages/opencode/src/session/schema.ts)),
   and `createUserMessage` builds parts with `PartID.make(part.id)` for given ids (`assign`, L694-697),
   triggers `chat.message` (L999-1009), logs `invalid user part before save` for a part that fails
   decoding (L1033-1045) and then persists every part with `sessions.updatePart` (L1047-1048,
   [session/prompt.ts](https://github.com/anomalyco/opencode/blob/v1.18.35/packages/opencode/src/session/prompt.ts)).
   An id without the prefix (the first draft used `tkm-...`) would fail that validation. Parts
   are saved after the hook returns, so the context persists as a synthetic part, which the readers
   exclude from prompts. Applying the reply is wrapped in `try`/`catch`, so a throw can never fail
   the prompt. Notice: `client.tui.showToast({body:{message,
   variant:"info"}})`, errors ignored. Pi `before_agent_start`: return `{message:{customType:
   "tokenimum", content:context, display:false}}`; notice via `ctx.ui.notify(notice, "info")`.
   Any spawn failure, timeout, non-zero exit, empty or invalid stdout leaves the host untouched.
5. **Timeouts and size.** `prompt` 3,000 ms, every other event 1,000 ms. opencode races Bun `$`
   (`.cwd(directory).quiet().nothrow()`, stdin redirected from a `Response`) against a timer,
   cleared once the race settles. `$` (`shell.d.ts`) has no abort signal or kill handle, so a timed-out
   `tkm-hook` is not killed; it ends by its own read timeout (SPEC 6.2). The opencode shim also
   fails open above 100,000 bytes of payload, like Pi. Its
   plugin runner has no hook timeout and awaits `chat.message` and `tool.execute.after` in
   sequence. The `event` hook is not awaited by opencode. Pi uses the `timeout` option of
   `pi.exec`. `tool_done` is awaited (not fire-and-forget); the 5 ms daemon read timeout of
   `tkm-hook` bounds it in practice. Spawning without awaiting remains a possible later
   optimisation (SPEC 14 overhead budget).
6. **Header and hash.** The first two lines of every shim are
   `// tokenimum <version> <host> shim. Generated by tkm init; do not edit.` and
   `// tokenimum-sha256: <hex>`, the SHA-256 over every byte after the second newline. Templates
   `shims/opencode.ts` and `shims/pi.ts` carry `{{VERSION}}` and `{{HASH}}`, are embedded with
   `include_str!` and rendered by `tkm_hosts::shim::render(host, version)` (the version is the
   crate version at install time). A rendered file has at most 40 lines and only `import type`
   imports (erased at load, no runtime dependency). `verify_shim(bytes) -> ShimStatus {Ok,
   HashMismatch, NotOurs}` is exposed for `tkm doctor`.
7. **Install and uninstall.** Paths: opencode `<agent_home>/plugins/tokenimum.ts` (agent home
   `~/.config/opencode`), project `<repo>/.opencode/plugins/tokenimum.ts`; Pi
   `<agent_home>/extensions/tokenimum.ts` (agent home `~/.pi/agent`), project
   `<repo>/.pi/extensions/tokenimum.ts`. Targets go through `writer::safe_target`, so a symlink
   (file or parent) that resolves outside the agent home or repository is refused. Writes are
   atomic (temporary file, rename), idempotent (an identical file is left untouched) and honour
   `dry_run`. An untouched shim of an older version is replaced. Install refuses an edited shim
   (`HashMismatch`) and a foreign file (`NotOurs`) with `AdapterError::UnsafeTarget` and leaves
   them. Uninstall removes a file only when the header parses for this host and the hash verifies;
   an edited shim is refused with `UnsafeTarget`; a foreign file or another host's shim is left
   alone and reported as unchanged. No backup is written (a fresh file, or a file proven ours).
   Instructions: opencode none; Pi "Run /reload in Pi; project scope needs project trust."
8. **Prune targets.** opencode `ConfigFileKey { path: "opencode.json", key: "permission" }`;
   Pi `ConfigFileKey { path: ".pi/settings.json", key: "defaultTools" }` (project settings override
   user settings; `-name` entries remove a tool: pi-settings.md).
9. **Pi `turn_end` is `agent_end`.** Pi fires `turn_end` once per model response and `agent_end`
   once per run of a prompt (SPEC 6.11 listed `turn_end`). tokenimum's turn is one prompt, so the
   shim forwards `agent_end` as `turn_end` and the adapter ignores native `turn_end`. `agent_end`
   can precede an automatic retry; a repeated `turn_end` is harmless to the daemon.
   `before_agent_start` with `messageType: "custom"` (a run started by an extension message) has no
   person prompt and maps to nothing.
10. **opencode `turn_end` is `session.idle`; `session.created` is not trusted alone.** `opencode run`
    creates its session through the same service, so the event is expected, but the daemon must
    still create a session on the first event it sees for an unknown id (idle-fold fallback).
11. **Call mapping.** opencode `tool.execute.after` carries `args` and `output.{output, metadata}`
    with no duration. `exit_code` is `metadata.exit` when present (bash); a non-zero exit is not
    an error. `is_error` is true only for a truthy `metadata.error` (opencode skips this hook for
    failed tools; the reader sees them as `state.status = "error"`). Edits are recorded for
    `write`, `edit` and `multiedit` from `filePath` (or `path`), relative to the session directory
    and only when inside it; `write` with `metadata.exists == false` is `Create`. `patch` and
    `apply_patch` hide paths in patch text and are not parsed. Pi `tool_result`: `toolName`,
    `input`, `content`, `isError`; edits from `write`/`edit` `path` as `Modify` (Pi does not say
    whether a file was created); no exit code (Pi reports none).
12. **opencode reader.** Opens `opencode.db` with `SQLITE_OPEN_READ_ONLY` (a directory source uses
    `<dir>/opencode.db`) and selects `session.id = source.session_id`; a missing session is
    `InvalidPayload`. Turns come from user messages (text parts that are neither `synthetic` nor
    `ignored`), calls from `tool` parts, ordered by `(time_created, id)`. Usage comes from assistant
    `tokens`: `input` already excludes cache, `output` is `output + reasoning`, `cache.read` and
    `cache.write` map to cache read and write; usage is also kept per `modelID`. A tool part with
    status `error` is a call with `is_error`; `exit_code` is `state.metadata.exit`; the duration is
    `time.end - time.start`. Records are messages plus parts of the session; unreadable JSON and
    unknown part types are skipped. Only damage is skipped: a row whose JSON does not parse, is not an object or has no string
    `type`; a message without a `role`. The `Part` union of the v1.18.35 SDK (text, subtask,
    reasoning, file, tool, step-start, step-finish, snapshot, patch, agent, retry, compaction) and
    types added later are valid and kept; the reader interprets text and tool parts and ignores the
    rest. More than 20% skipped marks the session partial. The tool catalog lists tools in first-use order (built-ins; a non-built-in name with an
    underscore is `Mcp` with no server, since `<server>_<tool>` cannot be split reliably).
13. **Deferred: legacy `storage/` JSON tree and `opencode export <id>` fallback.** A source
    directory without `opencode.db` is `AdapterError::Unsupported`. v1.18.35 (the verified
    version) uses the database; supporting the older layout needs fixtures from an older install
    that are not available. Fold-only mode (run B) reports the gap and the user keeps live shims.
14. **Pi reader.** Parses session JSONL v2/v3 (a missing header is `InvalidPayload`; version 1 is
    `Unsupported`). Entries form a tree; the active branch is the path from the last entry that has
    an `id` back to the root through `parentId`; every record with an `id` joins the tree, so a
    parent link passes through entry types the reader does not interpret, but the leaf is the last
    interpreted conversation entry (never a `usage` entry). Abandoned branches contribute no turns,
    calls or usage: the ledger counts the active context, an intended choice (their tokens were
    billed, but belong to history the person left). Standalone `usage` entries are spend outside
    any conversation (cache warming) and count for the session on any branch. Usage comes from
    assistant messages, tool-result `usage` (nested model work, attributed to the turn and to the
    model of the latest assistant message, so the per-model totals always sum to the session
    total), standalone `usage` entries and summary usage on the branch; Pi's `reasoning` is inside `output`. Calls pair `toolCall` blocks
    with `toolResult` messages by id. Unknown entry types and unreadable lines are skipped (same 20%
    rule); unknown message roles are ignored, not skipped (the docs tell consumers to tolerate
    them). The tool catalog is built from `toolsAdded` of system messages.
15. **Pi project settings path** is `<repo>/.pi/settings.json` (pi-settings.md: project settings
    override agent-directory settings; resource paths in project settings resolve from the project
    `.pi` directory).

## Consequences

- E4 must accept `--host-event` and `--payload`, pass `Request.cwd` and print the reply verbatim.
- Changing a template changes its hash for all installs; `tkm doctor` reports an old but untouched
  shim as `Ok` (the hash covers the body it shipped with) and `tkm init` upgrades it.
- A hand-edited shim is never removed or overwritten automatically.
