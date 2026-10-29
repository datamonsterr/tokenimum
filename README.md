# tokenimum

tokenimum learns from finished coding-agent sessions and uses that experience to cut repeated
token waste in later sessions. It learns repository facts ("run `python -m pytest`, not
`pytest`"), prunes tools that a repository never uses, and redirects turns that are stuck in a
failure loop.

- Harnesses: Claude Code, Codex, Antigravity (`agy`), opencode, Cursor, Kimi Code, a DeepSeek
  harness, and Pi.
- Fast: a Rust hook client (`tkm-hook`) and daemon (`tkmd`); hooks answer in under 3 ms.
- Cheap judgments: OpenRouter Decisions models with typed answers, about $0.00001 per call.
- Standalone: works on its own. superorchestrator and otherskill integrations are optional and
  enabled only when detected.
- Easy to run and inspect: `tkm init`, `tkm status`, `tkm top`, `tkm why <event>`, optional local
  web dashboard.
- Entirely safe Rust: `#![forbid(unsafe_code)]` in every crate, audited dependencies.

## Status

**E0 foundation in progress:** workspace, contracts, and CI. Read the [specification](SPEC.md),
[implementation plan](docs/plan/EPICS.md), and [contribution guide](CONTRIBUTING.md).

## Install

Download the Linux musl archive for your machine (`x86_64` or `aarch64`) from the
[GitHub Releases](https://github.com/datamonsterr/tokenimum/releases), verify its `.sha256` file,
and install the three binaries in `~/.local/bin`. Then run `tkm init` to configure detected
harnesses. See the [release installation guide](docs/release.md) for the full commands and
checksum details.

## Repository layout

| Path | Content |
|---|---|
| `SPEC.md` | full specification |
| `.agents/skills/` | agent skills for developing tokenimum, with sources and licenses ([index](.agents/skills/README.md)) |
| `.claude/skills` | symlink to `.agents/skills` for Claude Code |
| `NOTICE` | attribution, including microsoft/TokenEconomizer as the design origin |

## License

Apache-2.0. See [LICENSE](LICENSE) and [NOTICE](NOTICE).
