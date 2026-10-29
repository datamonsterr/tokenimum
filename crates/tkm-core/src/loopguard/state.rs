// Portions ported from microsoft/TokenEconomizer (tokeneconomizer/loopguard/state.py, commit d0675a3),
// Copyright (c) Microsoft Corporation, licensed under the Apache License, Version 2.0.
//! Compact per-call spiral signals.
use super::{
    shell_edits::{basename, shell_edit_targets},
    verification,
};
use crate::model::Call;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
/// Serializable counts for a segment or a whole prompt-turn.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct SpiralState {
    /// Completed calls, including non-failing content reads.
    pub turns: u32,
    /// Edits per basename, matching TE's signal unit.
    pub edit_counts: BTreeMap<String, u32>,
    /// Ordered edit basenames.
    pub edit_order: Vec<String>,
    /// Outputs containing error markers and eligible to fail.
    pub error_outputs: u32,
    /// Verification commands seen.
    pub test_runs: u32,
    /// Stable normalized failure fingerprints and counts; never raw output.
    pub fail_sigs: BTreeMap<String, u32>,
    /// Failures repeating a previous fingerprint.
    pub repeated_fail_sigs: u32,
}
impl SpiralState {
    /// Fold a normalized call once; shell edits and normalized edits are deduplicated.
    pub fn update(&mut self, call: &Call) {
        self.turns = self.turns.saturating_add(1);
        let mut names: Vec<String> = call
            .edits
            .iter()
            .filter_map(|edit| edit.path.to_str())
            .map(basename)
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
            .collect();
        if verification::is_shell(call) {
            names.extend(shell_edit_targets(&call.args_digest));
            if verification::counts_test_run(&call.args_digest) {
                self.test_runs = self.test_runs.saturating_add(1);
            }
        }
        let mut seen = Vec::new();
        for name in names {
            if seen.contains(&name) {
                continue;
            }
            seen.push(name.clone());
            let count = self.edit_counts.entry(name.clone()).or_default();
            *count = count.saturating_add(1);
            self.edit_order.push(name);
        }
        let failed = verification::reported_failure(call);
        let content = matches!(
            call.tool.to_ascii_lowercase().as_str(),
            "read" | "view" | "grep" | "glob"
        );
        let can_fail = if content {
            failed == Some(true)
        } else {
            failed != Some(false)
        };
        if can_fail {
            if let Some(line) = verification::error_line(&call.output_digest) {
                self.error_outputs = self.error_outputs.saturating_add(1);
                let signature = failure_signature(line);
                let count = self.fail_sigs.entry(signature).or_default();
                if *count > 0 {
                    self.repeated_fail_sigs = self.repeated_fail_sigs.saturating_add(1);
                }
                *count = count.saturating_add(1);
            }
        }
    }
    /// Maximum edits to a basename.
    #[must_use]
    pub fn edit_churn(&self) -> u32 {
        self.edit_counts.values().copied().max().unwrap_or_default()
    }
    /// Number of edited basenames.
    #[must_use]
    pub fn distinct_edit_files(&self) -> u32 {
        u32::try_from(self.edit_counts.len()).unwrap_or(u32::MAX)
    }
    /// Fraction of failing outputs repeating a previous failure.
    #[must_use]
    pub fn stagnation(&self) -> f64 {
        let total = self
            .fail_sigs
            .values()
            .fold(0_u32, |sum, n| sum.saturating_add(*n));
        if total == 0 {
            0.0
        } else {
            f64::from(self.repeated_fail_sigs) / f64::from(total)
        }
    }
}

// TE strips numbers, addresses and paths before hashing the first error line.
// A fixed FNV-1a fingerprint replaces MD5 to keep the port dependency-free;
// equality/repetition semantics, rather than MD5 bytes, are the public contract.
fn failure_signature(line: &str) -> String {
    let normalized = pattern!(r"0x[0-9a-f]+|\d+|/\S+")
        .map_or_else(|| line.into(), |re| re.replace_all(line, ""));
    let hash = normalized
        .trim()
        .bytes()
        .fold(0xcbf2_9ce4_8422_2325_u64, |hash, byte| {
            (hash ^ u64::from(byte)).wrapping_mul(0x0000_0100_0000_01b3)
        });
    format!("{hash:016x}")
}
