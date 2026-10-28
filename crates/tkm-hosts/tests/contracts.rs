#![forbid(unsafe_code)]

use std::collections::BTreeMap;

use serde::{Serialize, de::DeserializeOwned};
use tkm_core::model::{RepoKey, Session, Usage};
use tkm_hosts::{
    HostAdapter, InstallContext, InstallReport, NoopAdapter, PruneTarget, SessionSource,
};
use tkm_proto::{EventKind, Host, PROTOCOL_VERSION, ReplyAction, Request};

const HOSTS: [Host; 8] = [
    Host::Claude,
    Host::Codex,
    Host::Agy,
    Host::Cursor,
    Host::Kimi,
    Host::Deepseek,
    Host::Opencode,
    Host::Pi,
];
const EVENTS: [EventKind; 6] = [
    EventKind::SessionStart,
    EventKind::Prompt,
    EventKind::ToolBefore,
    EventKind::ToolDone,
    EventKind::TurnEnd,
    EventKind::SessionEnd,
];

fn source() -> SessionSource {
    SessionSource {
        agent_home: "/synthetic/agent".into(),
        path: "/synthetic/agent/absent-session.jsonl".into(),
        session_id: "synthetic-session".into(),
        repo_key: RepoKey("synthetic-origin".into()),
        cwd: "/synthetic/repo".into(),
    }
}

#[test]
fn host_adapter_is_usable_as_a_send_sync_trait_object() {
    let adapter: Box<dyn HostAdapter + Send + Sync> = Box::new(NoopAdapter::new(Host::Claude));
    assert_eq!(adapter.host(), Host::Claude);
    assert_eq!(adapter.prune_target(), PruneTarget::ReportOnly);
}

#[test]
fn noop_adapter_ignores_every_event_and_renders_exact_fallback_for_every_action() {
    for host in HOSTS {
        let adapter = NoopAdapter::new(host);
        for event in EVENTS {
            let request = Request {
                version: PROTOCOL_VERSION,
                host,
                event,
                host_event: None,
                payload: serde_json::Value::Null,
                env: BTreeMap::new(),
                cwd: None,
            };
            assert_eq!(adapter.map_event(&request).unwrap(), None);
            for action in [
                ReplyAction::Nothing,
                ReplyAction::Inject {
                    text: "context".into(),
                },
                ReplyAction::Notice {
                    text: "notice".into(),
                },
                ReplyAction::InjectAndNotice {
                    inject: "context".into(),
                    notice: "notice".into(),
                },
            ] {
                assert_eq!(adapter.render_reply(&request, &action), host.noop_reply());
            }
        }
    }
}

#[test]
fn noop_adapter_reads_empty_session_preserving_source_identity_without_io() {
    let source = source();
    for host in HOSTS {
        let adapter = NoopAdapter::new(host);
        let expected = Session {
            agent_home: source.agent_home.clone(),
            session_id: source.session_id.clone(),
            repo_key: source.repo_key.clone(),
            cwd: source.cwd.clone(),
            ..Session::new(host)
        };
        assert_eq!(adapter.read_session(&source).unwrap(), expected);
        assert_eq!(expected.usage, Usage::default());
    }
}

#[test]
fn noop_adapter_install_and_uninstall_change_no_files_in_both_modes() {
    for host in HOSTS {
        let adapter = NoopAdapter::new(host);
        for dry_run in [false, true] {
            let context = InstallContext {
                agent_home: "/synthetic/absent-agent".into(),
                repo_root: Some("/synthetic/absent-repo".into()),
                dry_run,
            };
            let expected = InstallReport {
                dry_run,
                ..InstallReport::default()
            };
            assert_eq!(adapter.install(&context).unwrap(), expected);
            assert_eq!(adapter.uninstall(&context).unwrap(), expected);
            assert_eq!(adapter.prune_target(), PruneTarget::ReportOnly);
        }
    }
}

fn round_trip<T: Serialize + DeserializeOwned + PartialEq + std::fmt::Debug>(
    expected: &T,
) -> Result<(), serde_json::Error> {
    let bytes = serde_json::to_vec(expected)?;
    let actual: T = serde_json::from_slice(&bytes)?;
    assert_eq!(&actual, expected);
    Ok(())
}

#[test]
fn adapter_support_types_serde_round_trip() {
    round_trip(&source()).unwrap();
    round_trip(&InstallContext {
        agent_home: "/synthetic/agent".into(),
        repo_root: None,
        dry_run: true,
    })
    .unwrap();
    round_trip(&InstallReport {
        changed_files: vec!["/synthetic/repo/.claude/settings.local.json".into()],
        instructions: vec!["synthetic trust step".into()],
        dry_run: true,
    })
    .unwrap();
    for target in [
        PruneTarget::ReportOnly,
        PruneTarget::SettingsDenyList {
            path: ".claude/settings.local.json".into(),
            key: "permissions.deny".into(),
        },
        PruneTarget::ConfigFileKey {
            path: "opencode.json".into(),
            key: "permission".into(),
        },
    ] {
        round_trip(&target).unwrap();
    }
}
