# Codewhale session idle observation

Checked 2026-10-29 against the [official hook reference](https://github.com/codewhale-hq/Codewhale/blob/main/docs/HOOKS.md). The `session_idle` observer receives a JSON state transition and session identity through environment variables. The fixture `fixtures/codewhale/hooks/session_idle.synthetic.json` is the documented example, not a recorded payload. Codewhale is not installed in this environment, so a real interactive TUI payload could not be captured. The documentation says `codewhale exec` does not fire hooks.
