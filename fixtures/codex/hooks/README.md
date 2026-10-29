# Codex hook payload fixtures

Codex 0.164.0 skips untrusted hooks, so payloads could not be dumped from a probe without the
trust-bypass flag (not used; see `docs/verified/claude-codex.md`). These payloads follow the field
tables of https://developers.openai.com/codex/hooks (checked 2026-10-29) and reuse the session id,
turn id, commands and outputs of the real local probe rollout in `../sessions/2026/10/29/`.
Required fields match the generated input schemas in
https://github.com/openai/codex/tree/main/codex-rs/hooks/schema/generated (`tool_response` is any
JSON value there). Bash and apply_patch `tool_response` strings are the model-facing outputs
copied from the probe rollout; `PostToolUse-mcp.json` uses the MCP call-result shape from the docs.
