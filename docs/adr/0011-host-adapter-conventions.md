# ADR 0011: Host adapter, installer and digest conventions (E5A)

Date: 2026-10-29. Status: accepted.

## Context

E5A implements the first real `HostAdapter`s (Claude Code, Codex). E4 (the `tkm-hook` binary and
daemon), E5B and E6 depend on choices made here: how hook entries are registered and
recognised, how settings files are edited without disturbing foreign entries, and how calls are
digested. Verified host facts are in `docs/verified/claude-codex.md`.

## Decisions

### Hook command line

Installers register `tkm-hook --host <host> --event <event>` where `<host>` is `Host::as_str` and
`<event>` is `EventKind::as_str` (`session_start`, `prompt`, `tool_done`, `turn_end`,
`session_end`). The binary is found on `PATH`. Claude Code uses exec form:
`"command": "tkm-hook", "args": ["--host", "claude", "--event", "prompt"]`. Codex has no `args`,
so `"command": "tkm-hook --host codex --event prompt"`. E4 implements this command line.

### Ownership

A handler is tokenimum's when its program is `tkm-hook` (the command's basename, before any
argument) and its arguments contain `--host <this host>`. tokenimum always writes its handler in
a matcher group of its own; a group is tokenimum's only when every handler in it is tokenimum's.
Uninstall removes tokenimum's groups and nothing else; events left with an empty array are
removed only when tokenimum created them.

### Byte-preserving JSON edits (`tkm_hosts::writer`)

Settings writers do not re-serialise the document. They validate it with `serde_json`, locate
byte spans with a small span scanner, and splice text in or out. Every byte outside the inserted
or removed spans is unchanged, whatever the file's formatting. Inserted text follows the
indentation detected at the insertion point.

- Backup: before the first write, copy the file to `<file>.tkm-bak` unless that backup exists.
- Uninstall: after removing tokenimum's spans, if the result parses equal to the backup, write
  the backup's exact bytes and delete the backup.
- Symlinks: a target that is a symlink resolving outside the agent home (user scope) or the
  repository (project scope) is refused with `AdapterError::UnsafeTarget`.
- Writes are atomic (temporary file in the same directory, then rename) and honour `dry_run`.
  Before the rename the writer re-reads the target; if it changed since the first read, it
  recomputes once from the new contents, and refuses on a second change, so a concurrent edit by
  the person is never overwritten.
- Accepted risk: containment is checked by path, then the write uses the path again. A process
  that can replace a directory inside the agent home or repository between the check and the
  write could redirect it. Closing this needs `openat`-style no-follow directory handles, which
  safe `std` does not offer; such a process can already edit the settings file itself.
- Install creating a file that did not exist writes an empty `<file>.tkm-bak` meaning "did not
  exist"; uninstall deletes the file only with that marker and only when an empty object remains.
- Accepted residual risks: a crash between writing the marker and the rename leaves a stale marker
  (a later uninstall may then delete a settings file that holds only `{}`), and two installers
  racing on the same file can remove the backup one of them relied on (no person data is lost;
  uninstall then leaves `{}` instead of deleting, or cannot restore exact original bytes).
- Idempotence: a second install computes no change and reports no changed files.

### Prune writer (Claude Code `permissions.deny`)

The caller passes the deny entries tokenimum owns (from its store) and the desired set. The writer
adds missing desired entries, removes owned entries no longer desired, never touches other
entries, and returns the new owned set. An entry the person already wrote is not owned. It
refuses a file tracked by git (`git ls-files --error-unmatch`) and a symlink out of the
repository, and appends the file to the path from `git rev-parse --git-path info/exclude` once.

### Replies

Claude Code: injection on `SessionStart`, `UserPromptSubmit`, `PostToolUse`,
`PostToolUseFailure`; never on `Stop` (it would continue the turn) or `SessionEnd`. Notice
(`systemMessage`) on every event except `SessionEnd`. Codex: injection on `SessionStart`,
`UserPromptSubmit`, `PostToolUse`; notice on these and `Stop`; nothing on `SessionEnd`. Text is
capped at 10,000 characters (Claude's hard cap; for Codex about the 2,500-token
`additionalContextLimit` the installer sets) on a `char` boundary. JSON is compact, keys in the
order `hookSpecificOutput`, `systemMessage`. A dropped part falls back to the remaining part or
to the empty no-op.

### Digests

`args_digest` and `output_digest` are compact, deterministic text: for shell tools the command
string; otherwise compact JSON of the input with object keys sorted. Output digests keep the
first line starting with an error marker (`error`, `Error`, `Exit code`, `fatal`, `panicked`) if
any, else the last non-empty line. Both are truncated to 512 characters. Live events and log
readers use the same functions (`tkm_hosts::digest`), so E2 sees equal digests for the same call.
Digests are not redacted; E3 redacts before any external call. Codex `apply_patch` updates digest by path (and move destination) only, because live patch hunks and rollout unified diffs are not comparable. Two different updates of one file therefore share a digest; consumers that count repeats (E2 loop guard, E3 digests) must not treat equal `apply_patch` digests alone as a stuck loop.
