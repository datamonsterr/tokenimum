# E10.6 replay benchmark verification

Verified 2026-10-29 against current official documentation. No private transcript, credential
file, model request or local Cargo compilation was used for verification.

| Surface | Official source and checked contract |
|---|---|
| Claude headless | [Headless](https://code.claude.com/docs/en/headless), also fetched as `headless.md`: `-p` accepts stdin, stream JSON is supported, resumed result costs include earlier turns. Normal print mode loads hooks/MCP; bare mode would invalidate this experiment. |
| Claude CLI | [CLI reference](https://code.claude.com/docs/en/cli-reference), fetched as `cli-reference.md`: `--model`, `--output-format stream-json`, `--verbose`, `--session-id` (UUID), `--resume`, `--effort`, `--permission-mode`. Prompts are sent through stdin, not argv. |
| Claude authentication | [Official CI instructions](https://code.claude.com/docs/en/github-actions): repository secret `ANTHROPIC_API_KEY`, or `CLAUDE_CODE_OAUTH_TOKEN` for OAuth. The workflow supports either. No login file is copied into the bundle. |
| Claude installation | [Setup](https://code.claude.com/docs/en/setup#install-a-specific-version), fetched as `setup.md`: native install script accepts an exact version; `DISABLE_AUTOUPDATER=1` prevents background updates. Bundle supplies the pinned version; the runner verifies `claude --version`. |
| Claude hooks | [Hook reference](https://code.claude.com/docs/en/hooks): settings hook groups and command handlers. No new hook event or payload shape is introduced. The config guard matches the existing installer in `tkm-hosts/src/claude/install.rs` exactly. |
| Checkout action | [actions/checkout](https://github.com/actions/checkout): v7; `repository`, `ref`, `fetch-depth: 0`, `token`, `path` and `persist-credentials: false`. A private secondary repository needs its own repository access token. |
| Artifact action | [actions/upload-artifact](https://github.com/actions/upload-artifact): v7; exact file path, `if-no-files-found: error`, and `retention-days`. No directory glob includes private run state. |
| Dispatch workflow | [GitHub workflow syntax](https://docs.github.com/en/actions/reference/workflows-and-actions/workflow-syntax): typed choice/boolean dispatch inputs, expressions, job timeout, and `if: always()` cleanup. Namespace runner label follows this repository's existing CI. |
| Existing Rust dependencies | [clap 4.6.7](https://docs.rs/clap/latest/clap/), [derive reference](https://docs.rs/clap/latest/clap/_derive/), [serde 1.0.229](https://docs.rs/serde/latest/serde/), [serde_json 1.0.154](https://docs.rs/serde_json/latest/serde_json/), [thiserror 2.0.21](https://docs.rs/thiserror/latest/thiserror/), [tempfile 3.27.0](https://docs.rs/tempfile/latest/tempfile/): nested derive subcommands, hidden command, JSON values/serialization, typed errors and synthetic temporary files. All already belong to the workspace; no dependencies were added. |
| Git anchoring and outcomes | [git-log](https://git-scm.com/docs/git-log), [git-reflog](https://git-scm.com/docs/git-reflog), [git-clone](https://git-scm.com/docs/git-clone), [git-reset](https://git-scm.com/docs/git-reset), [git-write-tree](https://git-scm.com/docs/git-write-tree), [git-diff](https://git-scm.com/docs/git-diff): `%H`, `%ct` (committer Unix seconds), branch-specific histories, historical HEAD limitations, isolated clone reset and final tree/changed-file comparisons. |
| Encryption | [GnuPG operations](https://www.gnupg.org/documentation/manuals/gnupg/Operational-GPG-Commands.html), [esoteric options](https://www.gnupg.org/documentation/manuals/gnupg/GPG-Esoteric-Options.html), [I/O](https://www.gnupg.org/documentation/manuals/gnupg/GPG-Input-and-Output.html): symmetric AES256, batch mode, loopback pinentry, passphrase FD, output and decrypt. Passphrase travels on stdin, never argv. |
| Python orchestration | [subprocess](https://docs.python.org/3/library/subprocess.html), [argparse](https://docs.python.org/3/library/argparse.html), [tarfile](https://docs.python.org/3/library/tarfile.html): argv execution, explicit environments/stdin, timeouts, and `filter="data"` extraction. The bundle reader additionally rejects all links and parent traversal. Python >=3.12 is required. Standard library only. |
| Download | [curl manual](https://curl.se/docs/manpage.html): fail-on-error HTTPS download for the official installer. Private bundle URL goes only to the Python HTTPS downloader, with exception contents suppressed. |
| Workflow validation | [actionlint usage](https://github.com/rhysd/actionlint/blob/main/docs/usage.md): targeted workflow validation; custom Namespace runner label is supplied as an allowed self-hosted label. |

Local source contracts also checked: `ClaudeAdapter::read_session`, its now-shared checked timestamp parser, `SessionSource`, Claude hook
installer, `fold_only::skill::render`, `tkm init` home/host handling and service fallback, `tkmd`
SIGTERM shutdown/flush, and `tkm export` decision/ledger fields. The new `activity` export field
counts current-repository redirects and completed session folds, and contains no text payloads.

Live pricing is not asserted here. Historical pricing belongs to the user-reviewed encrypted
bundle, with one rate per token class and exact recorded model. Missing prices fail accounting.

Local validation: `cargo fmt --all --check`, `git diff --check`, six offline Python tests
(`python3 -m unittest discover -s scripts/tests -p test_replay_benchmark.py`), and a synthetic
GnuPG encrypt/decrypt/unpack round trip passed. actionlint v1.7.12 passed for the new workflow
(with the existing custom Namespace runner label allowed). Rust compilation, Clippy and the
13 synthetic Rust tests are reserved for controller-pushed CI. No live benchmark was run.
