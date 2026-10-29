# agy fixtures

Captured 2026-10-29 from agy 1.4.9 on the maintainer's machine with `agy -p` in scratch
workspaces and a project `.agents/hooks.json` whose handlers wrote stdin to files. The real
`~/.gemini/config/hooks.json` was not modified. Redaction: home and workspace paths replaced by
`/home/user/...`; `thinking` text replaced by `<redacted>`. Conversation ids are random UUIDs.

- `hooks/`: real hook stdin payloads (`SessionStart`, `PreInvocation`, `PostInvocation`, `Stop`).
  `PreToolUse`/`PostToolUse` did not fire under `agy -p`, so there is no tool payload.
- `brain/<id>/.system_generated/logs/transcript_full.jsonl`: the file named by the hook
  `transcriptPath`. `1def34bb...` reads a file and runs `ls`; `14db3cb3...` writes, edits and runs
  a failing command; `049b40e1...` shows `SYSTEM_MESSAGE` / `EPHEMERAL_MESSAGE` steps produced by
  `injectSteps` replies.
- `conversations/schema.sql`: DDL of the conversation SQLite database (row payloads are
  undocumented protobuf blobs; no rows are committed).
- `config/hooks.json`: global hooks file shape with a foreign `otherskill` entry (flat handler list).
