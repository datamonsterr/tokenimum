# E9 integration surfaces (verified 2026-10-29)

Read from the installed tools on the author's machine. Tests never call the real tools.

## superorchestrator 0.1.0

- `superorchestrator --version` prints `superorchestrator 0.1.0`.
- Global options: `--config-dir <DIR>` (default `$SUPERORCHESTRATOR_CONFIG_DIR`, else the XDG
  config dir), `--json`, `-y`.
- `runs` subcommands: `list` (`--all` includes finished runs), `show`, `watch`, `cancel`. There
  is no `runs --json` listing; use `runs list --all --json`.
- `runs list --json` row keys: `account`, `branch`, `created_at`, `cwd`, `depth`, `error_kind`,
  `finished_at`, `harness`, `id`, `model`, `name`, `parent_run`, `session_id`, `state`,
  `worktree` (nullable).
- Account files `accounts/*.toml` keys: `name`, `harness`, `home`, `binary`, `enabled`, `cap`,
  `pools`, `priority`, `pass_env` (optional).
- `superorchestrator mcp` runs the MCP server on stdio. With `SUPERORCHESTRATOR_RUN_ID` and
  `SUPERORCHESTRATOR_RUN_TOKEN` set it acts for that run and adds `report_progress(step, status,
  note?)` and `ask_parent`. `report_progress` stores a `progress` message for the parent and
  wakes its `wait_agent`.
- Spawned agents get `SUPERORCHESTRATOR_RUN_ID`, `SUPERORCHESTRATOR_RUN_TOKEN`,
  `SUPERORCHESTRATOR_SOCKET`, `SUPERORCHESTRATOR_CONFIG_DIR` (source:
  `so-supervisor/src/runs/lifecycle.rs`).
- No usage-ingestion API for external programs (source: `so-usage`).

## otherskill (launcher release bqLzN9)

- No `--version`; `otherskill --help` answers.
- `select` = `find --json`. Options: `--agent claude|codex|antigravity|opencode`,
  `--prompt <literal text>`, `--timeout-ms N`, `--max-skills N`, `--config`, `--home`.
- Output (`RouteResult`): `status` (`ok|partial|failed`), `selected[]` (`name`, `description`,
  `path`, `category`, `logicalId`, `instanceId`, `categories`, `instances`, `paths`,
  `probability`, `confidence`), `categories`, `logicalCalls`, `diagnostics`.
- Config precedence: `--config` / `OTHERSKILL_CONFIG`, nearest ancestor `otherskill.config.json`
  or `.otherskill/config.json`, global `~/.config/otherskill/otherskill.config.json`.
- Key: `OPENROUTER_API_KEY`, else `~/.config/otherskill/credentials.json` (mode 0600).
