//! Pure per-checkout deny-list ownership plans.
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
/// Entries owned and explicitly kept per caller-supplied checkout key.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DenyState {
    /// Rules added by tokenimum per checkout.
    pub applied: BTreeMap<String, BTreeSet<String>>,
    /// Restored rules excluded from automatic application per checkout.
    pub kept: BTreeMap<String, BTreeSet<String>>,
}
/// Changes to a checkout's current deny rules.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DenyDelta {
    /// New rules to add.
    pub add: BTreeSet<String>,
    /// Owned rules to remove.
    pub remove: BTreeSet<String>,
}
/// Plan automatic application and update ownership.
pub fn apply(
    _state: &mut DenyState,
    _checkout: &str,
    _current: &BTreeSet<String>,
    _requested: &BTreeSet<String>,
) -> DenyDelta {
    DenyDelta::default()
}
/// Restore owned rules and remember restored entries as kept.
pub fn restore(
    _state: &mut DenyState,
    _checkout: &str,
    _current: &BTreeSet<String>,
    _requested: &BTreeSet<String>,
) -> DenyDelta {
    DenyDelta::default()
}
/// Change persistent keep overrides.
pub fn set_kept(
    _state: &mut DenyState,
    _checkout: &str,
    _add: &BTreeSet<String>,
    _remove: &BTreeSet<String>,
) {
}
