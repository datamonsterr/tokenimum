# Cursor fixtures

Cursor is not installed in this environment; all fixtures in this directory are synthetic and
constructed according to the official documentation (https://cursor.com/docs/hooks,
https://cursor.com/docs/agent/third-party-hooks, https://cursor.com/docs/cli/reference/permissions)
and community/forum reports on transcript formats and CLI behaviors. No real user data or secrets
are included.

- `hooks/`: synthetic payloads for each lifecycle event (`sessionStart`, `beforeSubmitPrompt`,
  `postToolUse`, `postToolUseFailure`, `stop`, `sessionEnd`), plus a CLI-style payload with an
  empty `conversation_id`.
- `transcripts/`: synthetic conversation JSONL transcripts matching the forum-reported format
  `{"role":"user"|"assistant","message":{"content":[...]}}`.
- `config/`: synthetic `hooks.json` containing foreign hooks and keys that must be preserved
  byte-for-byte by the installer.
