# tokenimum

**Help coding agents remember your project and waste fewer tokens.**

[Install](#installation) · [Use it in a project](#use-it-in-a-project) ·
[Update](#updating) · [Benchmarks](#benchmarks) · [Contributing](CONTRIBUTING.md)

## What is tokenimum?

tokenimum is a local tool that helps coding agents reuse what they learn about your project.
It saves useful facts from coding sessions and brings them into later sessions. It also detects
repeated failure loops and proposes removing tools the project does not use. For example,
an agent can learn that this repository needs `python -m pytest` instead
of `pytest`, rather than rediscovering the same fix each session.

This is an early preview. The CLI, daemon, eight host adapters, and offline tests are implemented;
real-world token savings and live decision-model quality have not yet been established.
See the [implementation status and known gaps](docs/spec-conformance.md).

## How does it work?

![A coding session sends events through hooks and shims to a local daemon, which learns repository facts and helps the next session. OpenRouter judgments are optional.](docs/images/how-tokenimum-works.png)

1. **Observe a session.** A host hook or shim sends events through `tkm-hook` to the local
   daemon, `tkmd`. The daemon tracks tool calls and reads supported session logs.
2. **Learn about the repository.** Completed and idle sessions are folded into evidence:
   useful commands, repository facts, recurring failures, and tool usage. SQLite stores this
   state locally, with facts scoped to the repository.
3. **Help the next session.** tokenimum supplies relevant facts, detects failure loops, and
   proposes tool pruning. Available actions depend on the host and preset. New repositories
   need session evidence before there is anything useful to reuse.
4. **Inspect the result.** Use `tkm` to see status, facts, decisions, and the evidence behind
   an action. The terminal monitor shows activity as it happens.

Optional OpenRouter Decisions requests judge small questions using redacted digests. The
`offline` preset disables these requests and keeps code-based evidence and deterministic
rules. Offline mode applies to tokenimum; your coding agent still uses its own model service.

### Supported coding agents

| Agent | Integration |
| --- | --- |
| Claude Code | Native hooks |
| Codex | Native hooks |
| Antigravity (`agy`) | Native hooks |
| Cursor | Native hooks |
| Kimi Code | Native hooks; event output capabilities vary |
| DeepSeek harness (Codewhale) | Native hooks |
| opencode | TypeScript plugin shim |
| Pi | TypeScript extension shim |

Host capabilities differ; native hooks do not imply that every host supports every action.
With shims disabled, opencode and Pi use session-log folding and instructions instead of live
interception. See [host verification](docs/verified/e5b-hosts.md) and
[opencode/Pi integration details](docs/verified/opencode-pi.md).

## Prerequisites

- At least one supported coding agent, installed and initialized under your user account.
- A project checkout. Run repository-specific commands from inside that project.
- Optional: an OpenRouter API key for model-assisted decisions. Start offline without a key.

The daemon uses a user service when available and can fall back to a detached process.
It does not require root.

## Installation

The installer downloads a release, verifies its archive and binary SHA-256 checksums, and installs
`tkm`, `tkm-hook`, and `tkmd` into `~/.local/bin`. No Rust toolchain, system musl, or `sudo` is needed.
It requires Bash and standard Linux tools (`tar`, `sha256sum`, and coreutils).

### Platform support

| Machine | Installer / release archive | Notes |
| --- | --- | --- |
| Linux, Intel/AMD 64-bit (`x86_64`) | Supported | Static musl binaries; works across glibc and musl distributions |
| Linux, ARM64 (`aarch64` / `arm64`) | Supported | Same installer; selects the ARM64 archive automatically |
| Windows with WSL2, x86_64 or ARM64 | Use the Linux installer inside WSL2 | Install and run your coding agent in the same WSL environment; WSL runtime is not yet verified |
| macOS, Intel or Apple Silicon | No release archive | Service support exists in source; native installation is not yet verified |
| Native Windows, 32-bit Linux, other architectures | No release archive | Use a supported Linux environment; there is no PowerShell installer |

### Quick install (current private repository)

Install the [GitHub CLI](https://cli.github.com/) and run `gh auth login` once with an account
that has repository access. Then run in Bash:

```bash
set -o pipefail
gh api -H 'Accept: application/vnd.github.raw+json' \
  'repos/datamonsterr/tokenimum/contents/install.sh?ref=main' |
  bash -s -- --method gh --version v0.0.0
```

The current release, `v0.0.0`, is a **preview**. Select it explicitly: GitHub's `latest` endpoint
only selects stable releases, and none is published yet.

Make the commands available in this terminal:

```bash
export PATH="$HOME/.local/bin:$PATH"
tkm --version
```

Add that `export` line to `~/.bashrc` (Bash) or `~/.zshrc` (Zsh) for future terminals.
Then [initialize tokenimum in your project](#use-it-in-a-project).

### Install with curl

For a **public** repository, the equivalent command is:

```bash
set -o pipefail
curl -fsSL https://raw.githubusercontent.com/datamonsterr/tokenimum/main/install.sh |
  bash -s -- --method curl --version v0.0.0
```

Anonymous curl downloads return 404 while this repository is private. To use curl now, install
`jq` and export `GH_TOKEN` with read access to this repository, then use the authenticated API:

```bash
set -o pipefail
curl -fsSL -H "Authorization: Bearer ${GH_TOKEN:?Export GH_TOKEN first}" \
  -H 'Accept: application/vnd.github.raw+json' \
  'https://api.github.com/repos/datamonsterr/tokenimum/contents/install.sh?ref=main' |
  bash -s -- --method curl --version v0.0.0
```

Both download methods support `--bin-dir /absolute/path` and `--version TAG`. For example,
append `--bin-dir "$HOME/bin"` to install elsewhere, and put that directory on `PATH`.
The installer prints its next steps and does not edit your shell files or agent configuration.
You can also download and inspect [install.sh](install.sh) before running `bash install.sh --help`.

### Install from release assets (manual or offline)

Download the matching archive **and** its adjacent `.sha256` file from
[Releases](https://github.com/datamonsterr/tokenimum/releases) while signed in. You can copy
these files to another machine with the same OS and architecture; the destination needs no network.

| Linux architecture | Archive |
| --- | --- |
| x86_64 | `tokenimum-x86_64-unknown-linux-musl.tar.gz` |
| ARM64 | `tokenimum-aarch64-unknown-linux-musl.tar.gz` |

In a new directory containing those two files, set `archive` to your filename, then run:

```bash
set -e
archive=tokenimum-x86_64-unknown-linux-musl.tar.gz
sha256sum -c "$archive.sha256"
tar -xzf "$archive"
sha256sum -c SHA256SUMS
mkdir -p "$HOME/.local/bin"
install -m755 tkm tkm-hook tkmd "$HOME/.local/bin/"
export PATH="$HOME/.local/bin:$PATH"
tkm --version
```

If you also copied `install.sh`, you can replace those steps with
`bash install.sh --archive "$PWD/tokenimum-x86_64-unknown-linux-musl.tar.gz"`
(use the ARM64 filename on ARM64); it verifies and installs without downloading.
For updates, stop the daemon first as described below. For build provenance and publication
details, see the [release guide](docs/release.md).

### Updating

1. Finish active coding sessions and stop the daemon. With a systemd user service, run
   `systemctl --user stop tkmd.service`. If `init` reported a detached process instead (for
   example, without systemd), run `pkill -u "$(id -u)" -x tkmd`; exit code 1 means none was running.
2. Re-run your installation command with the desired `--version TAG`, using the **same bin
   directory**. Choose a published tag from [Releases](https://github.com/datamonsterr/tokenimum/releases).
   Once stable releases exist, `--version latest` selects the newest stable release. Previews
   always need an explicit tag. Manual/offline users repeat the release-asset steps with the new files.
3. From your project, refresh integrations and start the daemon with your existing preset:

   ```bash
   tkm init --yes
   tkm doctor
   tkm status
   ```

   Reuse your original `--only`, `--home`, or `--no-shims` options if you customized initialization.
   Restart your coding agent after these checks.

Updates replace all three executables. They keep your configuration and local database; you do
not need to uninstall first. Read release notes for migration requirements and back up local data
before an upgrade. There is no `tkm update` command or automatic updater. `tkm --version` reports
the CLI version; `tkm doctor` checks daemon health after restart.

## Use it in a project

Open a terminal inside your project checkout. Preview the changes, then install using the
offline preset:

```bash
tkm init --dry-run --preset offline
tkm init --yes --preset offline
tkm doctor
tkm status
```

`init` detects agent homes and shows which configuration files it will change. It installs the
selected integrations and daemon service. Restart your coding agent after installation, then
work normally in the project. `doctor` checks daemon and integration health; a failed check
includes a diagnostic to investigate.

To select one agent, use its short name. For example, for an initialized Claude Code home:

```bash
tkm init --yes --only claude --preset offline
```

Other names are `codex`, `agy`, `cursor`, `kimi`, `deepseek`, `opencode`, and `pi`.
Use `tkm init --help` for custom agent homes and other installation options.

### Inspect what it learned

```bash
tkm status
tkm facts --brief
tkm prune
tkm config show --effective
tkm export > tokenimum-state.json
```

These commands report the current repository. A fresh installation may have no facts,
proposals, or actions yet. `export` omits stored argument/output digests by default.

After an action has been recorded, inspect its evidence:

```bash
tkm why last
```

This returns exit code 1 with an `unknown action event` message if there is no action yet.

For a live view, run this in an interactive terminal; press `q` to quit:

```bash
tkm top
```

### Choose a preset

| Preset | Behavior |
| --- | --- |
| `offline` | No decision-model requests; code evidence, deterministic rules, pruning proposals |
| `observe` | Collect evidence and report proposals; suppress fact injection and use shadow loop decisions |
| `balanced` | Default thresholds and evidence gates for facts, loop handling, and pruning |
| `aggressive` | Lower fact-confirmation threshold and allow pruning application, subject to protection rules |

To change the global preset:

```bash
tkm config preset offline
```

For model-assisted decisions, run interactive setup and choose a key source, then select
`balanced`. Setup can read `OPENROUTER_API_KEY`, reuse a key file, or accept hidden key entry.
Model actions require their configured evidence and calibration gates; adding a key does not
immediately authorize every action. Neither `observe` nor `balanced` is an offline preset.

The web dashboard requires a daemon built with the optional `web` feature. The preview
archives use the default build, so use `tkm top` with those binaries.

### Configuration and local data

On Linux, defaults are:

| Location | Purpose |
| --- | --- |
| `~/.config/tokenimum/config.toml` | User configuration |
| `.tokenimum.toml` in your project | Shared project configuration |
| `.tokenimum.local.toml` in your project | Local project overrides |
| `~/.local/share/tokenimum/tokenimum.db` | SQLite evidence and session state |
| `~/.local/share/tokenimum/errors.log` | Error log |

XDG directory overrides are supported. Configuration layers run from defaults and presets to
user, project, local-project, environment, and invocation flags. Some settings are restricted
at project scope. `tkm config show --effective` shows the resolved values and their sources.

Session state stays on your machine. If you enable decision models, tokenimum sends redacted
summaries to OpenRouter. Review [data handling](SPEC.md#84-request-rules) before enabling them.
superorchestrator and otherskill integrations are optional and activated when detected.

### Uninstall integrations

Stop the `tkmd` user service or detached process first. A running detached daemon prevents
complete removal; the CLI reports how to stop it. Preview removal, then remove tokenimum-owned
integration entries:

```bash
tkm uninstall --dry-run
tkm uninstall
```

See `tkm uninstall --help` to limit removal to one host.

## Benchmarks

The [main-branch benchmark run from October 30, 2026](https://github.com/datamonsterr/tokenimum/actions/runs/41000421547)
measured the following at commit `16ca349` on the `namespace-profile-linux-xl` CI runner:

| Measurement | Result | Method |
| --- | --- | --- |
| Native hook → daemon round trip | p50 **1.609 ms**, p99 **1.880 ms** | 500 runs after 20 warmups; recorded Claude tool event; decisions disabled |
| opencode shim round trip | p50 **7.702 ms**, p99 **9.833 ms** | Bun shim benchmark against a running daemon |
| Pi shim round trip | p50 **6.464 ms**, p99 **7.668 ms** | Bun shim benchmark against a running daemon |
| Daemon tool-event handling | **2.916–12.932 µs** | Three Criterion cases; excludes process startup |
| Fold a Claude fixture session | **8.417 ms** | Criterion; synthetic fixture data |
| Render a terminal-monitor pane | **0.086–0.398 ms** | Six Criterion cases |

These measure overhead on one runner, not token savings or live model latency. Results vary
by hardware, session size, and host. CI enforces native-hook p50 below 3 ms and p99 below 10 ms,
plus per-operation budgets and a 20% regression gate when a main baseline is available.

Run the deterministic decision benchmark from this repository checkout, without an API key:

```bash
tkm replay --bench fixtures/bench
```

The committed [decision report](docs/reports/m0-decision-bench.md) uses **synthetic model
responses**. Its agreement, latency, and costs validate benchmark plumbing; they do not measure
production models. No representative end-to-end token-saving percentage is available yet.
The [replay benchmark plan](docs/plan/replay-benchmark.md) describes the controlled comparison.

For performance reproduction, use the repository's
[Bench workflow](.github/workflows/bench.yml). Contributor builds, tests, and benchmarks run
in GitHub Actions under the [contribution rules](CONTRIBUTING.md).

## Development and reference

- [Specification](SPEC.md) and [architecture decisions](docs/adr/)
- [Implementation status](docs/spec-conformance.md) and [delivery plan](docs/plan/EPICS.md)
- [README verification record](docs/verified/readme.md)
- `crates/tkm-cli`, `crates/tkm-hook`, `crates/tkmd`: CLI, hook client, daemon
- `crates/tkm-core`, `crates/tkm-store`, `crates/tkm-hosts`: policies, storage, host adapters
- `fixtures/`, `scripts/`: offline fixtures and validation tools

## License

[Apache-2.0](LICENSE). See [NOTICE](NOTICE) for attribution, including the
microsoft/TokenEconomizer design origin.
