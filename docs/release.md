# Release and benchmark workflows

## Linux releases

The release workflow builds `tkm`, `tkm-hook`, and `tkmd` as statically linked musl binaries for
`x86_64-unknown-linux-musl` and `aarch64-unknown-linux-musl`. It uses pinned `cargo-zigbuild`
and Zig versions so both architectures can be built on the `namespace-profile-linux-xl` runner.
SPEC 13.2 requires static Linux musl releases; it does not specify macOS release targets.

Each target is built twice in clean target directories with `SOURCE_DATE_EPOCH=0`, incremental
compilation disabled, and Cargo/workspace paths remapped. The workflow compares SHA-256 hashes
and fails if they differ. It also requires `file` to identify every binary as statically linked;
on x86_64, `ldd` must report that the executable is not dynamic. A target archive contains all
three executables and a `SHA256SUMS` file. The release also attaches a SHA-256 checksum for each
archive and a JSON `cargo-geiger` report.

Tag pushes matching `v*` build and publish when the repository variable
`RELEASE_PUBLISH_ENABLED` is set to `true`. A manual `workflow_dispatch` defaults to dry-run and
builds the same artifacts; publishing requires dispatching on a version tag, setting `dry_run` to
false, and enabling the repository variable. Pull requests that change the workflow or Rust build
inputs run the build and geiger jobs without publishing. Heavy build and geiger work uses
`namespace-profile-linux-xl`; artifact download and release publication use
`namespace-profile-datamonster01`.

## Install from a GitHub Release

Choose the archive matching the machine architecture. On Linux, `x86_64` means a 64-bit Intel or
AMD machine and `aarch64` means 64-bit ARM. Download both the `.tar.gz` archive and its `.sha256`
file from the release assets, then verify and install:

```sh
sha256sum -c tokenimum-x86_64-unknown-linux-musl.tar.gz.sha256
tar -xzf tokenimum-x86_64-unknown-linux-musl.tar.gz
mkdir -p "$HOME/.local/bin"
install -m755 tkm tkm-hook tkmd "$HOME/.local/bin/"
```

For ARM64, use `tokenimum-aarch64-unknown-linux-musl.tar.gz` and its matching checksum file in
the commands above. Ensure `$HOME/.local/bin` is in `PATH`, then run:

```sh
tkm init
```

The archive's `SHA256SUMS` checks the three executable files. The adjacent `.sha256` release asset
checks the archive itself. The binaries are statically linked and require no system musl runtime.

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
