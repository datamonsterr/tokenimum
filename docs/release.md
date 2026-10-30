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

## Benchmark workflow

The benchmark workflow compares the Criterion placeholder and hyperfine's mean hook startup
against the latest cached main-branch results. Main pushes save a new immutable cache; PRs read
but never save it, and fail when either result exceeds 120% of the corresponding baseline.
A PR with no baseline passes; the gate can only be exercised after a successful main run has
saved one. Cache eviction likewise leaves no baseline until main runs again. Shared runners are
noisy, especially for the current empty hook; E4.8 and E10.3 should replace the placeholder and
tune the gate and performance budgets. The hyperfine benchmark runs the hook with empty stdin;
it is not yet the daemon round trip or a p99 check.
