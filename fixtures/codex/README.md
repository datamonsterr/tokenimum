# Codex fixtures

- `sessions/2026/10/29/`: rollout from a real `codex exec` probe (codex-cli 0.164.0,
  2026-10-29, temporary `CODEX_HOME`), two turns (shell, failing shell, apply_patch).
  The `.jsonl.zst` file is the same rollout compressed with the zstd CLI, standing in for the
  older compressed rollouts Codex keeps. Redacted like the Claude fixtures.
- `hooks/`: hook payloads; see `hooks/README.md` for provenance.
- `config/`: `hooks.json` and `config.toml` with the shape of real files and foreign entries
  (otherskill, rtk, trusted hook state) that installers must preserve byte-for-byte.
