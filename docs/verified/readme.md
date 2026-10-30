# README verification

Verified on 2026-10-30 on Linux x86_64 using the actual `v0.0.0` release assets.
No local Cargo build, Rust test, Clippy, or benchmark compilation was run.

## Source and release provenance

- Release: <https://github.com/datamonsterr/tokenimum/releases/tag/v0.0.0>
- Tagged source: main commit `16ca349`.
- Artifact source: successful [Release run 40998364820](https://github.com/datamonsterr/tokenimum/actions/runs/40998364820),
  commit `2432208208c24e40903f9bca35a4a1cde27407c2`.
- Both commits resolve to Git tree `2839be92914e84a22143349a181eb92c615a77ec`.
- That release workflow built each architecture twice, compared binary hashes, and checked
  static linkage. Seven assets were published: two archives, two archive checksum files,
  and three cargo-geiger reports. `SHA256SUMS` is inside each archive.

## Installation

Extracted the first Bash block directly from README.md and executed it with `bash -euc`
in a temporary directory and temporary HOME. Only GitHub CLI authentication was reused
through `GH_CONFIG_DIR` for access to this private repository.

The block successfully downloaded the published x86_64 assets, checked the archive checksum,
extracted it, checked all three executable checksums, installed the binaries under the temporary
`~/.local/bin`, updated PATH, and printed `tkm --help`. Exit code: 0.
`file` independently identified all three executables as statically linked x86_64 ELF binaries.

ARM64 asset checksums and cross-compilation are covered by the release workflow. ARM64 runtime
execution and macOS installation were not tested here. The README does not advertise macOS
release archives.

## Commands and instructions

`python3 scripts/fresh_install.py --bin-dir /path/to/extracted/release` passed init, doctor,
and a recorded hook round trip against a running daemon.

`python3 scripts/verify_readme.py --bin-dir /path/to/extracted/release` passed **18 command
checks** in an isolated HOME and Git repository, without credentials or live model requests:

- CLI and init help; offline dry-run and apply; doctor and status.
- Explicit Claude selection; fact listing, pruning proposals, effective configuration.
- `why last`: verified the expected exit code 1 when no action exists. The README explains
  this precondition rather than implying an empty repository already contains an action.
- JSON export: parsed the output and wrote the documented export file.
- Global offline preset selection and deterministic fixture decision replay.
- Interactive `tkm top`: rendered through a PTY and exited successfully on `q`.
- Uninstall help, dry-run, and apply. A detached daemon must stop before complete removal;
  the script signals only the daemon belonging to its temporary HOME. The README explains this.

The smoke script also runs in the existing CI fresh-install job. The verification does not
establish live host callback behavior for eight installed agents, live OpenRouter quality,
interactive key entry, or systemd/launchd behavior. Those limits are separate from the tested
offline CLI examples.

## Benchmark claims

All reported overhead numbers were read from the logs of
[main Bench run 41000421547](https://github.com/datamonsterr/tokenimum/actions/runs/41000421547)
at `16ca349`, including the sample count and warmups in its workflow. They are dated CI
measurements, not new local measurements or promises for other hardware. Decision fixtures
and their report explicitly identify synthetic model responses. The README makes no numerical
token-saving claim.

## Diagram and formatting

The architecture illustration was generated and edited with the built-in imagegen tool.
It was visually inspected for readable labels and the full 1 → 2 → 3 → 4 → 5 → 1 cycle,
with a separate optional OpenRouter branch. The asset is committed at
[how-tokenimum-works.png](../images/how-tokenimum-works.png), with the exact
[generation and edit prompts](../images/how-tokenimum-works.prompt.md).

Local Markdown file/image targets and heading anchors were checked, as was `git diff --check`.
