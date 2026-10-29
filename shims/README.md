# Shim templates

`opencode.ts` and `pi.ts` are the templates `tkm init` renders into the opencode plugin and the Pi
extension (SPEC 6.11, ADR 0025). Cargo does not build them; `crates/tkm-hosts/src/shim.rs` embeds
them with `include_str!`.

Rules:

- At most 40 lines per rendered file, no logic beyond: serialise the event, spawn `tkm-hook` with a
  hard timeout, apply the reply. Only `import type` imports.
- Lines 1 and 2 are the header. `{{VERSION}}` and `{{HASH}}` are filled in by `shim::render`; the
  hash covers every byte after line 2, so edit the body freely but keep the two header lines.
- Changing a template needs the matching adapter and contract-test change (`tests/shim.rs`).
- Never install a rendered shim by hand into a real `~/.config/opencode` or `~/.pi`; use `tkm init`.
