# Claude Code fixtures

- `hooks/`: hook payloads dumped from a real `claude -p` probe (Claude Code 2.1.312, 2026-10-29)
  with every event of SPEC table 6.2 plus `PostToolUseFailure`. Paths redacted to `~`.
- `sessions/`: the probe's transcript (two turns: Bash, failing Bash, Write, Read, Edit). Long
  system/tool text in non-conversation records and thinking signatures are replaced with
  `[redacted...]` markers; record types and structure are unchanged.
- `sessions/skills-otherskill.jsonl`: skill evidence records. Record and attachment shapes
  (`skill_listing`, `hook_additional_context` with an otherskill selection, `Skill` and `Read`
  `tool_use`, and the metadata types `bridge-session`, `permission-mode`, `ai-title`,
  `frame-link`, `artifact-*`, `dev-mods`, `continued-in`) were copied key for key from real
  Claude Code 2.1 transcripts on 2026-11-03. All values are synthetic.
- `config/`: settings files with the shape of real user settings and foreign hook entries
  (otherskill, mem-admission, rtk) that installers must preserve byte-for-byte.
