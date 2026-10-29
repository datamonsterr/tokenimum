# Codex hook payload fixtures

Codex 0.164.0 skips untrusted hooks, so payloads could not be dumped from a probe without the
trust-bypass flag (not used; see `docs/verified/claude-codex.md`). These payloads follow the field
tables of https://developers.openai.com/codex/hooks (checked 2026-10-29) and reuse the session id,
turn id, commands and outputs of the real local probe rollout in `../sessions/2026/10/29/`.
`tool_response` shapes for Bash and apply_patch are the model-facing output strings.
