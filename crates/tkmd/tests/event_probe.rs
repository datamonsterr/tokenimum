#![forbid(unsafe_code)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::pedantic)]
mod common;
use tkm_proto::{EventKind, Host};

#[test]
fn event_policy_preserves_native_event_independence_and_recovers() {
    let store = tkm_store::Store::open_in_memory().unwrap();
    store.record_host_event_probe(Host::Claude, "PostToolUse", EventKind::ToolDone, true, 1).unwrap();
    store.record_host_event_probe(Host::Claude, "PostToolUseFailure", EventKind::ToolDone, false, 1).unwrap();
    assert!(store.host_event_probes(Host::Claude).unwrap().iter().any(|r| !r.arrived));
    store.record_host_event_probe(Host::Claude, "PostToolUseFailure", EventKind::ToolDone, true, 2).unwrap();
    assert!(store.host_event_probes(Host::Claude).unwrap().iter().all(|r| r.arrived));
}
