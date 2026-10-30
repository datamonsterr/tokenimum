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

## Original manual installation

For the README at `addd67e`, extracted the first Bash block and executed it with `bash -euc`
in a temporary directory and temporary HOME. Only GitHub CLI authentication was reused
through `GH_CONFIG_DIR` for access to this private repository.

The block successfully downloaded the published x86_64 assets, checked the archive checksum,
extracted it, checked all three executable checksums, installed the binaries under the temporary
`~/.local/bin`, updated PATH, and printed `tkm --help`. Exit code: 0.
`file` independently identified all three executables as statically linked x86_64 ELF binaries.

ARM64 asset checksums and cross-compilation are covered by the release workflow. ARM64 runtime
execution and macOS installation were not tested here. The README does not advertise macOS
release archives.

## Release installer and update instructions

The installation section now uses root `install.sh`. Its verification entry point is the
`installer` CI job on [PR #26](https://github.com/datamonsterr/tokenimum/pull/26/checks).
The job runs shell syntax validation and `scripts/tests/test_installer.py`, covering:

- Linux architecture selection, ARM64 aliases, and unsupported OS/architecture errors.
- Explicit preview tags and the default stable-release URL.
- GitHub CLI, anonymous curl, and token-authenticated asset API downloads.
- Download failure, corrupt archive, corrupt binary, and missing binary checksum rejection.
- Local archive installation without downloads, paths containing spaces, invalid options,
  and replacement of an executable while its previous inode is running.

The live step fetches the script using both authenticated bootstrap commands, using the PR head
as `ref` while the file is not yet on main. It compares fetched bytes to the checked-out script,
downloads the actual `v0.0.0` release with `gh` and authenticated curl, checks the installed
executables, and runs `tkm --version`. Only the CLI implements a version flag; `tkmd --version`
starts a daemon and must not be used as a version probe. The existing isolated fresh-install
and README scripts then exercise init, doctor, a hook round trip, and the documented CLI examples
against the installed preview. Tests and smoke checks run in Actions, without local Cargo builds.

The README documents stopping the daemon before replacement, refreshing integrations with the
existing preset and initialization options, and checking daemon health afterward. The installer
changes only the bin directory; it does not edit shell startup files or initialize integrations.
Moving installations to another bin directory and schema rollback are outside this procedure.

Checked repository state on 2026-10-30: private, with a single `v0.0.0` prerelease and both
Linux archives/checksums. Anonymous public-URL downloads cannot be tested against this private
repository; their URL selection is covered offline. ARM64 selection and release build checks
do not establish ARM64 runtime verification. WSL2 and macOS runtime behavior were not tested.

Command/API references checked on 2026-10-30:

- [GitHub CLI release download](https://cli.github.com/manual/gh_release_download)
  and [API raw responses](https://cli.github.com/manual/gh_api).
- [Release asset API](https://docs.github.com/en/rest/releases/assets#get-a-release-asset)
  for authenticated `application/octet-stream` downloads.
- [Repository contents API](https://docs.github.com/en/rest/repos/contents#get-repository-content)
  for raw script bootstrap, and [latest release semantics](https://docs.github.com/en/rest/releases/releases#get-the-latest-release).
- [curl options](https://curl.se/docs/manpage.html) for HTTP errors, redirects, TLS-only
  transport, connection timeout, and transfer timeout.

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
