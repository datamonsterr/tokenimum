#![forbid(unsafe_code)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::pedantic)]

use std::{
    collections::BTreeMap,
    fs,
    os::unix::fs::PermissionsExt,
    sync::Arc,
    time::{Duration, Instant},
};
use tkm_integrations::{
    EscalationEvent, Escalator,
    detect::{AdapterHealth, SystemClock},
    superorchestrator::SuperorchestratorEscalator,
};
use tkm_proto::Host;

#[test]
fn mcp_timeout_kills_descendants_holding_stdout() {
    let dir = tempfile::tempdir().unwrap();
    let marker = dir.path().join("survived");
    let binary = dir.path().join("superorchestrator");
    fs::write(
        &binary,
        format!(
            "#!/bin/sh\n(/bin/sleep 7; printf survived > '{}') &\nwait\n",
            marker.display()
        ),
    )
    .unwrap();
    fs::set_permissions(binary, fs::Permissions::from_mode(0o755)).unwrap();
    let adapter = SuperorchestratorEscalator::new(
        dir.path().as_os_str().to_owned(),
        dir.path().to_path_buf(),
        Arc::new(AdapterHealth::new(Arc::new(SystemClock))),
    );
    let env = BTreeMap::from([
        ("SUPERORCHESTRATOR_RUN_ID".into(), "run".into()),
        ("SUPERORCHESTRATOR_RUN_TOKEN".into(), "token".into()),
    ]);
    adapter.observe_session(Host::Claude, "s", dir.path(), &env);
    let start = Instant::now();
    assert!(!adapter.try_escalate(&EscalationEvent {
        session_id: "s".into(),
        ..Default::default()
    }));
    assert!(start.elapsed() < Duration::from_secs(6));
    std::thread::sleep(Duration::from_secs(3));
    assert!(!marker.exists(), "the descendant survived its parent");
}
