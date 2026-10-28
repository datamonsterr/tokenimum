# ADR 0010: Shared protocol, model, and host-adapter contracts

Date: 2026-10-29. Status: accepted.

## Context

E1–E6 need one contract for persistence, pure mechanisms, decisions, the synchronous hook,
the asynchronous daemon, host log readers, installers, and shims. SPEC 5, 6.1–6.4, 7.3,
13, 14, and 16 define the boundaries; ADR 0001 supplies defaults for unverified hosts.
E0.2 and E0.3 freeze these contracts before downstream implementation. The request-based
rendering signature and per-model usage fields were added during E0 review, before the freeze.

## Wire types and host identities (`tkm-proto`)

`Host` is a non-exhaustive enum with lowercase wire/CLI names `claude`, `codex`, `agy`,
`cursor`, `kimi`, `deepseek` (Codewhale), `opencode`, and `pi`. `as_str` and `FromStr`
round-trip those exact names; unknown names, aliases, and different casing return
`ParseHostError { name: String }`. `Host` lives in `tkm-proto`, and is re-exported by
`tkm-core::model`, so the tiny hook never depends on core mechanisms or host readers.

`Host::noop_reply` returns exact stdout without a newline: `{}` for agy and Cursor,
empty for every other host. Empty shim output means no mutation. The agy fallback is
the SPEC's inferred default pending E5B verification; E0 does not claim new verification.

`EventKind` is non-exhaustive: `session_start`, `prompt`, `tool_before`, `tool_done`,
`turn_end`, `session_end`. `as_str`/`FromStr` support installer `--event` values and return
`ParseEventKindError { name: String }` for an unknown value. Read timeouts are 300 ms for
session start, 1,500 ms for prompts, and 5 ms for tool/end events. SPEC 6.2 describes
Codewhale's `tool_call_before` injection but gives it no separate timeout; `tool_before`
therefore shares the 5 ms tool budget and delivers already prepared context only.
`CONNECT_TIMEOUT` is 2 ms. These methods do not start timers or perform I/O.

| Type | Public fields |
| --- | --- |
| `Request` | `version: u16`, `host: Host`, `event: EventKind`, `host_event: Option<String>`, `payload: serde_json::Value`, `env: BTreeMap<String, String>`, `cwd: Option<String>` |
| `Reply` | `version: u16`, `action: ReplyAction`, `stdout: String` |

The request retains native event names and raw stdin JSON, using JSON `null` when no stdin
exists. Only adapter-selected environment markers are forwarded, never the whole environment.
Wire working directories are UTF-8 strings from the host; normalized filesystem paths use
`PathBuf`. Non-UTF-8 paths need an explicit host policy in later adapters, not lossy conversion
hidden in this contract.

`ReplyAction` is a non-exhaustive internally tagged enum, with JSON `kind` and snake-case
names: `nothing` (default), `inject { text: String }`, `notice { text: String }`, and
`inject_and_notice { inject: String, notice: String }`. **The daemon's host adapter renders
`stdout`; the hook prints it verbatim and never interprets `action`.** Host caps and event
restrictions belong in `render_reply`, including no Codex injection at turn/session end.
No action blocks, cancels, or terminates a turn. Unsupported injection/notice surfaces use
the no-op fallback and daemon logging.

## Framing, limits, errors, and versions

Each frame contains a four-byte big-endian unsigned `u32` length followed by exactly that
many UTF-8 JSON body bytes. The length excludes the header. One synchronous read consumes
one frame and leaves the next frame unread. Writers do not flush automatically.

- `MAX_REQUEST_FRAME = 8 * 1024 * 1024` body bytes: tool outputs can be large, while the
  bound prevents an untrusted header from requesting an arbitrary allocation.
- `MAX_REPLY_FRAME = 64 * 1024` body bytes: enough for ordinary host JSON around capped
  context (Claude caps context at 10,000 characters). JSON escaping can expand text, so
  this is an independent byte cap, not a guarantee that every host-capped string fits.
  Later rendering/hook code must handle oversize replies by failing open.
- `PROTOCOL_VERSION: u16 = 1` appears as the required `version` field in both messages.
  Framing deliberately preserves any decoded version, so peers can detect mismatches.
  Each receiver checks equality before processing and falls back on mismatch; framing
  does not silently upgrade or interpret an incompatible peer.

The public sync API uses `std::io`: `write_frame<W: Write, T: Serialize + ?Sized>(writer,
&value, max) -> Result<(), FrameError>` and `read_frame<R: Read, T: DeserializeOwned>(reader,
max) -> Result<T, FrameError>`. Owned transports or mutable references both work.
Pure helpers are `encode_frame(&value, max) -> Result<Vec<u8>, FrameError>`,
`decode_len([u8; 4], max) -> Result<usize, FrameError>`, and
`decode_body::<T>(&[u8]) -> Result<T, FrameError>`. The daemon can use those helpers with
Tokio without introducing an async runtime into the hook or this crate. Async transports
must validate `decode_len` before allocating/reading the body.

The synchronous reader checks the length before allocating or reading any body byte.
Encoding serializes the body before checking its size, then emits no bytes on a
serialization/size failure; the send-side limit is not a serialization memory budget.
`FrameError` is non-exhaustive and uses `thiserror`:

| Variant | Meaning |
| --- | --- |
| `Oversize { len: usize, max: usize }` | Advertised or serialized body exceeds the supplied byte limit. |
| `Truncated` | EOF before the full header or body, including a completely empty stream. |
| `Empty` | Zero-length header or empty body passed directly to `decode_body`. |
| `Malformed(serde_json::Error)` | Serialization, UTF-8, JSON syntax, trailing data, or target shape failure. |
| `LengthOverflow(TryFromIntError)` | Checked wire/native length conversion cannot represent the body length. |
| `Io(std::io::Error)` | Other transport error, preserving the source and error kind. |

After a read/write error the connection is discarded; a write I/O failure can have emitted
part of a frame. Pure body decoding does not re-check a frame size or protocol version.

## Pure normalized models (`tkm-core::model`)

These types contain only data and pure helpers; there is no clock, filesystem, network,
or async runtime access. Paths below are `PathBuf` values supplied by the caller; `Option`
means missing/unknown rather than a guessed value.

| Type | Public fields / variants |
| --- | --- |
| `RepoKey(pub String)` | Transparent opaque string; E1 computes normalized identity shared by worktrees. Empty default means unresolved. |
| `Timestamp(pub i64)` | Transparent signed Unix milliseconds; epoch default, negative values permitted, no chrono dependency. |
| `Session` | `host: Host`, `agent_home: PathBuf`, `session_id: String`, `repo_key: RepoKey`, `cwd: PathBuf`, `started: Timestamp`, `ended: Option<Timestamp>`, `turns: Vec<Turn>`, `tool_catalog: Vec<ToolDef>`, `usage: Usage`, `usage_by_model: BTreeMap<String, Usage>`, `partial: bool`, `skipped_records: u64`, `total_records: u64`. |
| `Turn` | `prompt: String`, `calls: Vec<Call>`, `stopped_by: StopReason`, `model: Option<String>`, `usage: Usage`, `usage_by_model: BTreeMap<String, Usage>`. |
| `Call` | `tool: String`, `args_digest: String`, `output_digest: String`, `exit_code: Option<i32>`, `is_error: bool`, `duration_ms: Option<u64>`, `edits: Vec<FileEdit>`. |
| `FileEdit` | `path: PathBuf` relative to the repo, `kind: EditKind`, `via: EditVia`. Readers validate containment; the type does no path I/O or validation. |
| `EditKind` | Non-exhaustive `create`, `modify`, `delete`. |
| `EditVia` | Non-exhaustive `tool`, `shell`, keeping shell edits visible to E2.1. |
| `Usage` | `input: u64`, `output: u64`, `cache_read: u64`, `cache_write: u64`; zero default. |
| `StopReason` | Non-exhaustive `own_stop`, `escalated`, `redirected`, `interrupted`, `error`, `unknown` (default). Redirects and escalations remain distinct for E2. |
| `ToolDef` | `name: String`, `kind: ToolKind`, `server: Option<String>` for an MCP server when known. |
| `ToolKind` | Non-exhaustive `builtin`, `mcp`, `skill`, `other` (default). |

`Usage::add(self, other)` and the `+` operator saturate each field independently at
`u64::MAX`. Input means uncached input, excluding cache read/write tokens; readers must
normalize host totals so E2.7 does not double count. Usage remains the total; per-model
usage enables pricing mixed-model sessions, and unknown model identity remains absent.
The new fields default to `None` or empty maps when reading older JSON. Digests are compact descriptions,
not implicitly safe to export: E3 redacts them before external decisions.

`Turn`, `Call`, `ToolDef`, and `Usage` implement `Default` and deserialize missing fields
to their defaults. `Session::new(host)` creates empty identity/paths/collections, epoch
start, unknown end, zero usage/counts, and `partial = false`; readers fill metadata before
using it as evidence. Sessions count every record and skipped unknown/malformed records.
Readers set `partial` above 20% skipped; partial sessions are used for the ledger only.
The contract holds these fields without performing reader policy calculations.

`HookEvent` also lives in core because E2's pure mechanisms consume normalized live input.
Putting it in `tkm-hosts` would make core depend on I/O adapters or duplicate that event shape.
It has public fields `host: Host`, `kind: EventKind`, `session_id: String`,
`turn_id: Option<String>`, `repo_key: Option<RepoKey>`, `cwd: Option<PathBuf>`,
`timestamp: Option<Timestamp>`, `prompt: Option<String>`, `call: Option<Call>`,
`usage: Usage`, `stopped_by: Option<StopReason>`, `tool_catalog: Vec<ToolDef>`.
`HookEvent::new(host, kind, session_id)` fills only required identity and defaults the rest.
Usage on a live event is an increment, not a cumulative counter; cumulative host usage
must be converted into deltas before aggregation. Optional metadata and catalog updates
avoid forcing a log read on the hook's hot path.

## Host boundary (`tkm-hosts`)

`HostAdapter: Send + Sync` is object safe and supports `Box<dyn HostAdapter + Send + Sync>`.
All methods take `&self` and are synchronous:

| Method | Result / responsibility |
| --- | --- |
| `host()` | `Host` selected by the adapter. |
| `map_event(&Request)` | `Result<Option<HookEvent>, AdapterError>`; `None` intentionally ignores an event; real adapters validate version/host/required metadata, without I/O. |
| `render_reply(&Request, &ReplyAction)` | `String`, using the native event name to enforce host caps/rules and no-op fallback, without I/O. |
| `read_session(&SessionSource)` | `Result<Session, AdapterError>`; log reader with skipped counts/partial marking; foreign databases read-only. |
| `install(&InstallContext)` | `Result<InstallReport, AdapterError>`; writes only tokenimum-owned entries, or computes a dry run. |
| `uninstall(&InstallContext)` | `Result<InstallReport, AdapterError>`; removes only tokenimum-owned entries, or computes a dry run. |
| `prune_target()` | `PruneTarget` describing a verified surface, or report-only. |

Mapping/rendering are memory-only. The daemon executes I/O-heavy readers/installers with
`spawn_blocking`; the trait does not impose boxed futures, async trait machinery, or a Tokio
dependency. Implementations can use synchronization internally if needed, while the
contract remains available to synchronous replay/CLI consumers.

| Supporting type | Public fields / variants |
| --- | --- |
| `SessionSource` | `agent_home: PathBuf`, `path: PathBuf` (log file, DB, or runtime directory), `session_id: String`, `repo_key: RepoKey`, `cwd: PathBuf`; empty defaults. |
| `InstallContext` | `agent_home: PathBuf`, `repo_root: Option<PathBuf>` (`None` = user scope), `dry_run: bool`; empty paths, user scope, and false by default. Callers explicitly select dry run. |
| `InstallReport` | `changed_files: Vec<PathBuf>` (written/removed or proposed paths), `instructions: Vec<String>` (manual trust steps, etc.), `dry_run: bool`; empty/false defaults. |
| `PruneTarget` | Non-exhaustive internally tagged enum (`kind`): `settings_deny_list { path: PathBuf, key: String }`, `config_file_key { path: PathBuf, key: String }`, `report_only` (default). Paths are relative to the repo; dotted keys/prefixes are interpreted by the host's writer. |
| `AdapterError` | Non-exhaustive `InvalidPayload { message: String }`, `Unsupported { operation: String }`, `Io(std::io::Error)`, `Malformed(serde_json::Error)` with chained sources via `thiserror`. |

Prune metadata supports Claude/Cursor `permissions.deny`, opencode `permission`, and Pi
`defaultTools` conventions. Codex, agy, Kimi, and Codewhale remain report-only until their
surfaces are verified. Metadata authorizes no write by itself. Real writers in E5/E6 must
preserve foreign entries, back up the first write, respect dry runs, refuse committed
configs and escaping symlinks, and follow SPEC 7.3/13.4 exclusion rules.

`NoopAdapter::new(host)` is the public reference/test implementation, not a real host mapping.
It always maps `None` (even for invalid/foreign requests), renders `host.noop_reply()` for
every event/action, and reads an empty session while preserving source host/home/session/repo/cwd.
It does no I/O, installs/removes nothing, echoes the dry-run flag in empty reports, and
always returns `PruneTarget::ReportOnly`. It intentionally demonstrates safe fallback,
not host validation or rendering semantics that belong to E5/E6.

## Compatibility and evidence

Public enums expected to grow are `#[non_exhaustive]`; downstream matches need a fallback.
This permits Rust variant additions but does not make old JSON readers understand new tags:
unknown wire tags fail deserialization and the hook/daemon must fail open. Public data types
derive `Debug`, `Clone`, `PartialEq`, `Eq`, `Serialize`, and `Deserialize`; small value types
also implement `Copy`. Source-bearing error enums cannot implement `Clone`/`Eq` or serde
without losing error fidelity, so they derive `Debug`/`Error` instead.

All enum wire names use stable snake_case; action/prune enums use a `kind` tag. Newtypes use
transparent serde representation. Structs have public fields and defaults or constructors
where sensible; they are **not** `#[non_exhaustive]`, so other workspace crates can construct
them with literals. Later epics extend contracts additively; a breaking change requires a
**new ADR**. Preserve existing names/meaning and use optional/defaulted new serialized fields.
Adding public struct fields requires adjusting workspace literals and is an explicitly
coordinated extension, not source compatibility for arbitrary external literal users.

Tests were committed before each implementation. E0.2 evidence is in
`crates/tkm-proto/tests/framing.rs`: request/reply round trips, oversize encode and early
header rejection, truncation, garbage/wrong-shape/invalid UTF-8/trailing JSON, zero length,
I/O faults, version visibility, exact host/timeouts, and arbitrary payload string property
round trips. E0.3 evidence is in `crates/tkm-core/tests/model.rs` and
`crates/tkm-hosts/tests/contracts.rs`: model/support serde round trips, saturating addition,
identity representation/defaults, trait-object use, and every host/event/action no-op path.
Rustdoc examples cover sync framing and usage addition. CI executes all compiled checks;
local validation is limited to formatting, metadata, unsafe-policy, whitespace, and review.

Dependency APIs checked on 2026-10-29 at locked versions:
[serde 1.0.229](https://docs.rs/serde/1.0.229/serde/),
[serde_json 1.0.154](https://docs.rs/serde_json/1.0.154/serde_json/),
[thiserror 2.0.21](https://docs.rs/thiserror/2.0.21/thiserror/), and
[proptest 1.12.0](https://docs.rs/proptest/1.12.0/proptest/).
