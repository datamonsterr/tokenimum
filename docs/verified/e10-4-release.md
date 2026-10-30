# E10.4 release tool and action verification

Checked 2026-10-29 against upstream documentation:

- `cargo-zigbuild` 0.24.0 README documents installation with `cargo install --locked`, use of
  Zig as linker, and that `*-musl` targets are the choice for fully static Linux executables:
  <https://docs.rs/crate/cargo-zigbuild/0.24.0/source/README.md>
- `cargo-geiger` documents locked installation and unsafe-usage reporting:
  <https://github.com/geiger-rs/cargo-geiger#readme>
  Its JSON output-format option is documented in the generated CLI help reference:
  <https://docs.rs/cargo-geiger/latest/cargo_geiger/args/constant.HELP.html>
- `actions/checkout@v7` is shown by the official README:
  <https://github.com/actions/checkout>
- `actions/upload-artifact@v7` is the current artifact action major; release history:
  <https://github.com/actions/upload-artifact/releases>
- `actions/download-artifact@v8` supports downloading all matching artifacts into one directory
  with `merge-multiple: true`; release history and usage:
  <https://github.com/actions/download-artifact/releases>
  <https://github.com/actions/download-artifact#download-all-artifacts>

The workflow pins its external build tools to cargo-zigbuild 0.24.0, Zig 0.17.0, and
cargo-geiger 0.14.0. Action references use explicit released major versions.

Version availability checked against the registries on 2026-10-29:

- <https://crates.io/crates/cargo-zigbuild/0.24.0>
- <https://pypi.org/project/ziglang/0.17.0/>
- <https://crates.io/crates/cargo-geiger/0.14.0>
