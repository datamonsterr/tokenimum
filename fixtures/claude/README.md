# Claude Code fixtures

- `hooks/`: hook payloads dumped from a real `claude -p` probe (Claude Code 2.1.312, 2026-10-29)
  with every event of SPEC table 6.2 plus `PostToolUseFailure`. Paths redacted to `~`.
- `sessions/`: the probe's transcript (two turns: Bash, failing Bash, Write, Read, Edit). Long
  system/tool text in non-conversation records and thinking signatures are replaced with
  `[redacted...]` markers; record types and structure are unchanged.
- `config/`: settings files with the shape of real user settings and foreign hook entries
  (otherskill, mem-admission, rtk) that installers must preserve byte-for-byte.
