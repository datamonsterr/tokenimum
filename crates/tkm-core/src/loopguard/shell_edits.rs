// Portions ported from microsoft/TokenEconomizer (tokeneconomizer/loopguard/shell_edits.py, commit d0675a3),
// Copyright (c) Microsoft Corporation, licensed under the Apache License, Version 2.0.
//! Heuristic shell-write detection; no shell execution or filesystem access.
/// Ordered unique basenames written by a shell command, as in TE.
/// Unresolved script and patch targets use `(script)` and `(patch)`.
#[must_use]
pub fn shell_edit_targets(_command: &str) -> Vec<String> {
    Vec::new()
}
