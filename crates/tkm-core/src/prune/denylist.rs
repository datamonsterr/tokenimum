// Portions ported from microsoft/TokenEconomizer (tokeneconomizer/plugin/denylist.py),
// Copyright (c) Microsoft Corporation, licensed under the Apache License, Version 2.0.
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
/// Plan automatic application. Commit the returned state only after the adapter
/// successfully applies the delta; checkout identity is caller-supplied.
#[must_use]
pub fn apply(
    state: &DenyState,
    checkout: &str,
    current: &BTreeSet<String>,
    requested: &BTreeSet<String>,
) -> (DenyState, DenyDelta) {
    let mut next = state.clone();
    let kept = state.kept.get(checkout);
    let add: BTreeSet<_> = requested
        .iter()
        .filter(|name| !current.contains(*name) && !kept.is_some_and(|items| items.contains(*name)))
        .cloned()
        .collect();
    next.applied
        .entry(checkout.to_owned())
        .or_default()
        .extend(add.iter().cloned());
    (
        next,
        DenyDelta {
            add,
            remove: BTreeSet::new(),
        },
    )
}
/// Restore requested owned rules and remember them as kept. Foreign rules are untouched.
/// Commit the returned state only after the adapter successfully applies the delta.
#[must_use]
pub fn restore(
    state: &DenyState,
    checkout: &str,
    current: &BTreeSet<String>,
    requested: &BTreeSet<String>,
) -> (DenyState, DenyDelta) {
    let mut next = state.clone();
    let owned = next.applied.entry(checkout.to_owned()).or_default();
    let targets: BTreeSet<_> = owned.intersection(requested).cloned().collect();
    let remove = targets.intersection(current).cloned().collect();
    for name in &targets {
        owned.remove(name);
    }
    next.kept
        .entry(checkout.to_owned())
        .or_default()
        .extend(targets);
    (
        next,
        DenyDelta {
            add: BTreeSet::new(),
            remove,
        },
    )
}
/// Change persistent keep overrides; removals take precedence over additions.
pub fn set_kept(
    state: &mut DenyState,
    checkout: &str,
    add: &BTreeSet<String>,
    remove: &BTreeSet<String>,
) {
    let kept = state.kept.entry(checkout.to_owned()).or_default();
    kept.extend(add.iter().cloned());
    for name in remove {
        kept.remove(name);
    }
}
