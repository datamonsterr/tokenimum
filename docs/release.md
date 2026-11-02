# Release and benchmark workflows

## Release builds

The release workflow builds `tkm`, `tkm-hook`, and `tkmd` as statically linked musl binaries for
`x86_64-unknown-linux-musl` and `aarch64-unknown-linux-musl`. It uses pinned `cargo-zigbuild`
and Zig versions so both architectures can be built on the `namespace-profile-linux-xl` runner.
macOS builds use native GitHub-hosted macOS runners for `aarch64-apple-darwin` and
`x86_64-apple-darwin`. SPEC 13.2 requires static Linux musl releases.

Each target is built twice from a clean target directory (macOS reuses the same path) with `SOURCE_DATE_EPOCH=0`, incremental
compilation disabled, and Cargo/workspace paths remapped. The workflow compares SHA-256 hashes
and fails if they differ. On Linux it also requires `file` to identify every binary as statically linked;
on x86_64, `ldd` must report that the executable is not dynamic. A target archive contains all
three executables and a `SHA256SUMS` file. The release also attaches a SHA-256 checksum for each
archive and a JSON `cargo-geiger` report.

Tag pushes matching `v*` build and publish when the repository variable
`RELEASE_PUBLISH_ENABLED` is set to `true`. A manual `workflow_dispatch` defaults to dry-run and
builds the same artifacts; publishing requires dispatching on a version tag, setting `dry_run` to
false, and enabling the repository variable. Pull requests that change the workflow or Rust build
inputs run the build and geiger jobs without publishing. Linux build and geiger work uses
`namespace-profile-linux-xl`, macOS builds use GitHub-hosted runners; artifact download and release publication use
`namespace-profile-datamonster01`.

## Install from a GitHub Release

See the [README installation instructions](../README.md#installation) for the platform matrix,
authenticated GitHub CLI and curl commands, and manual/offline installation. The root
[`install.sh`](../install.sh) selects the Linux or macOS architecture, downloads the archive and checksum,
verifies the archive and all three executables, and installs into `~/.local/bin` by default.
It stages replacements in the destination filesystem so an existing executable can keep running
until the daemon is restarted. No Rust toolchain or system musl runtime is needed. Windows users can run `install.ps1` to
install in their default WSL2 distribution, with authentication configured within WSL.

The installer is fetched from the default branch, independently of the binary release tag.
`--version TAG` selects an explicit release, including previews; `latest` selects the latest
stable release only. The initial preview must use `--version v0.0.0`. The repository is private:
anonymous curl URLs work only after it becomes public. For authenticated curl, the installer
resolves asset API URLs using `GH_TOKEN` (or `GITHUB_TOKEN`) and `jq`; GitHub CLI handles its own
authentication. `--archive /path/to/tokenimum-TARGET.tar.gz` uses local files without network.

Updates replace all three binaries and preserve local state. Follow the
[update procedure](../README.md#updating) to stop the old daemon, select the release, refresh
integrations, and check health. Moving an existing installation to another bin directory is
not part of this procedure: recorded service and hook paths must continue to resolve.

The CI `installer` job covers offline failure cases and real downloads of the initial preview
using both authentication methods. These small shell/Python checks use the existing
`namespace-profile-datamonster01` runner; Rust build and benchmark jobs retain their existing profiles.

## Initial preview

[v0.0.0](https://github.com/datamonsterr/tokenimum/releases/tag/v0.0.0) is an explicitly
marked prerelease of the current `0.0.0` workspace. It includes both Linux architectures,
archive checksums, per-binary checksums inside each archive, and cargo-geiger reports.
The release notes record artifact provenance and the matching source tree. Publication
reused the successful release-build artifacts; automatic publication remains controlled by
`RELEASE_PUBLISH_ENABLED`.

The repository is private. For authenticated downloads and tested installation commands,
see the [README installation section](../README.md#installation).

## Benchmark workflow

The [Bench workflow](../.github/workflows/bench.yml) runs Criterion benchmarks for tool-event
handling, prompt handling, session folding, and terminal rendering. It also measures native
hook round trips, Bun-based opencode/Pi shim round trips, and idle daemon memory/CPU.
Native hook round trips use a recorded tool event against a running daemon with decisions
disabled, with 20 warmups and 500 measured runs. The gate requires p50 below 3 ms and p99
below 10 ms. Empty-stdin hook startup is measured separately.

Main pushes save immutable baseline caches. Pull requests read the latest available baseline
and fail above 120% of the corresponding result, in addition to absolute performance budgets.
A missing or evicted baseline skips the relative comparison until main establishes another.

The README links a dated run and reports its measured overhead. The committed decision report
uses synthetic test-double responses and is not evidence of live model quality or token savings.
