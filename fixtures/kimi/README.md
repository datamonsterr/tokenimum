# Synthetic Fixtures for Kimi Code (kimi-cli 1.52.0)

Kimi Code is not installed locally in this environment. All fixtures in this directory
are completely synthetic and built strictly from the upstream source shapes
(`https://github.com/MoonshotAI/kimi-cli` at commit `9ab1286`, release 1.52.0):
- `src/kimi_cli/hooks/events.py` (hook event payload schemas)
- `src/kimi_cli/soul/context.py` (`context.jsonl` message records and checkpoints)
- `src/kimi_cli/wire/types.py` (`wire.jsonl` turn boundary and status update usage records)
- `src/kimi_cli/metadata.py` (session directory hash scheme `md5(work_dir)/<session_id>`)
- `src/kimi_cli/hooks/config.py` (`config.toml` `[[hooks]]` configuration table schema)

No real user data, credentials, or proprietary tokens are contained in these fixtures.
