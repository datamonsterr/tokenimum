//! Shared TOML editing helpers for the Kimi and Codewhale installers.

use toml_edit::{ArrayOfTables, RawString, Table};

const OWNED_MARKER: &str = "# tokenimum";

/// Returns the comment and blank lines of `table`'s prefix, minus the marker line we wrote.
fn foreign_prefix(table: &Table) -> String {
    table
        .decor()
        .prefix()
        .and_then(RawString::as_str)
        .unwrap_or_default()
        .split_inclusive('\n')
        .filter(|line| line.trim() != OWNED_MARKER)
        .collect()
}

/// Removes every table for which `owned` is true.
///
/// Foreign comments that sat above a removed table move to the prefix of the next remaining
/// table. Comments with no later table in the array are returned so the caller can place them.
pub(crate) fn remove_owned(arr: &mut ArrayOfTables, owned: impl Fn(&Table) -> bool) -> String {
    let old = std::mem::take(arr);
    let mut carry = String::new();
    for mut table in old {
        if owned(&table) {
            carry.push_str(&foreign_prefix(&table));
        } else {
            if !carry.is_empty() {
                let existing = table
                    .decor()
                    .prefix()
                    .and_then(RawString::as_str)
                    .unwrap_or_default()
                    .to_owned();
                table.decor_mut().set_prefix(format!("{carry}{existing}"));
                carry.clear();
            }
            arr.push(table);
        }
    }
    carry
}
