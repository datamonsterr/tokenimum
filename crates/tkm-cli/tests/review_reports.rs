#![forbid(unsafe_code)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::pedantic)]

mod support_report;
use support_report::Fixture;
use tkm_core::model::{Host, Timestamp};
use tkm_store::{FactRecord, FactState, NewEvent, NewSession, SessionKey};

const SECRET: &str = "sk-or-v1-review-fixture-credential";

fn secret_fact(f: &Fixture) -> i64 {
    let store = f.store();
    store.insert_candidate_fact(&FactRecord {
        repo_key: f.repo.clone(), fact_id: "secret-fact".into(), family: "env_value".into(),
        scope: "neutral".into(),
        text_params: serde_json::json!({"params": {"variable": "OPENROUTER_API_KEY", "good": {SECRET: 3}, "sample": SECRET}}),
        state: FactState::Candidate, score: 3.0, confirmations: 3, contradictions: 0,
        fire_count: 3, fire_sessions: 4, state_changed_at: None,
    }).unwrap();
    store
        .transition_fact(&f.repo, "secret-fact", FactState::Active)
        .unwrap();
    let session_id = store
        .upsert_session(&NewSession {
            key: SessionKey {
                host: Host::Claude,
                agent_home: "/fixture".into(),
                session_id: "secret-session".into(),
            },
            repo_key: f.repo.clone(),
            cwd: f.home.path().into(),
            started: Timestamp(1),
            ended: None,
            fold_status: "open".into(),
            shadow: false,
            integration_level: "hooks".into(),
        })
        .unwrap();
    store
        .insert_event(&NewEvent {
            kind: "fact.injected".into(),
            session_id,
            inputs: serde_json::json!({"fact_ids": ["secret-fact"]}),
            evidence_ids: vec![],
            explanation_template_id: "fact_injected".into(),
            params: serde_json::json!({}),
        })
        .unwrap()
}

#[test]
fn export_redacts_secret_object_keys_and_values_in_both_modes() {
    let f = Fixture::new();
    secret_fact(&f);
    let stored = f.store().get_fact(&f.repo, "secret-fact").unwrap().unwrap();
    assert_eq!(stored.text_params["params"]["good"][SECRET], 3);
    assert_eq!(stored.text_params["params"]["sample"], SECRET);
    for args in [&["export"][..], &["export", "--show-digests"][..]] {
        let out = f.run(args);
        assert!(out.status.success(), "{out:?}");
        let json: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
        assert!(!json.to_string().contains(SECRET), "{json}");
        assert!(json.to_string().contains("[redacted:openrouter]"));
    }
}

#[test]
fn fact_and_explanation_reports_redact_env_value_credentials() {
    let f = Fixture::new();
    let event = secret_fact(&f).to_string();
    for args in [
        vec!["facts"],
        vec!["facts", "--brief"],
        vec!["why", &event],
        vec!["status"],
        vec!["status", "--json"],
    ] {
        let out = f.run(&args);
        assert!(out.status.success(), "{args:?}: {out:?}");
        let text = String::from_utf8(out.stdout).unwrap();
        assert!(!text.contains(SECRET), "{args:?}: {text}");
        assert!(text.contains("[redacted:"), "{args:?}: {text}");
    }
}

#[test]
fn fact_reports_preserve_templates_while_redacting_secret_settings() {
    let f = Fixture::new();
    let event = secret_fact(&f).to_string();
    let claim = "Code that imports the project needs `OPENROUTER_API_KEY` set here: `OPENROUTER_API_KEY=[redacted:openrouter]` works.";
    for args in [
        vec!["facts"],
        vec!["facts", "--brief"],
        vec!["why", &event],
        vec!["status"],
        vec!["status", "--json"],
    ] {
        let out = f.run(&args);
        assert!(out.status.success(), "{args:?}: {out:?}");
        let text = String::from_utf8(out.stdout).unwrap();
        assert!(text.contains(claim), "{args:?}: {text}");
        assert!(!text.contains(SECRET), "{args:?}: {text}");
    }
}

#[test]
fn why_and_status_redact_stored_parameters_before_rendering_templates() {
    let f = Fixture::new();
    let stored = f.store().get_event(secret_fact(&f)).unwrap().unwrap();
    for nested in [false, true] {
        let values = serde_json::json!({
            "detail": format!("retry the failing command with {SECRET}"),
            "why": format!("recorded evidence {SECRET}"),
            "model_checks": format!("stored check {SECRET}"),
            "cost": SECRET,
            "undo": format!("remove {SECRET}"),
        });
        let params = if nested {
            serde_json::json!({"kind": "redirect", "values": values})
        } else {
            values
        };
        let event = f
            .store()
            .insert_event(&NewEvent {
                kind: "loop.redirect".into(),
                session_id: stored.session_id,
                inputs: serde_json::json!({}),
                evidence_ids: vec![],
                explanation_template_id: "redirect".into(),
                params,
            })
            .unwrap()
            .to_string();
        let out = f.run(&["why", &event]);
        assert!(out.status.success(), "{out:?}");
        let text = String::from_utf8(out.stdout).unwrap();
        for expected in [
            "What happened: tokenimum redirected the agent:",
            "retry the failing command with [redacted:openrouter]",
            "Why: recorded evidence [redacted:openrouter]",
            "Model checks: stored check [redacted:openrouter]",
            "Cost of those checks: [redacted:openrouter].",
            "Undo: remove [redacted:openrouter]",
        ] {
            assert!(text.contains(expected), "{text}");
        }
        assert!(!text.contains(SECRET), "{text}");
        let out = f.run(&["status", "--json"]);
        assert!(out.status.success(), "{out:?}");
        let report: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
        assert_eq!(
            report["repositories"][0]["last_actions"][0]["explanation"],
            "tokenimum redirected the agent: retry the failing command with [redacted:openrouter]"
        );
        assert!(!report.to_string().contains(SECRET), "{report}");
    }
}

#[test]
fn why_session_without_event_defaults_to_last() {
    let f = Fixture::new();
    secret_fact(&f);
    let out = f.run(&["why", "--session", "secret-session"]);
    assert!(out.status.success(), "{out:?}");
    assert!(String::from_utf8_lossy(&out.stdout).contains("What happened:"));
}

#[test]
fn status_all_resolves_each_checkout_and_labels_global_fallback() {
    let f = Fixture::new();
    let store = f.store();
    let mut keys = Vec::new();
    for (name, mode) in [("first", "off"), ("second", "shadow")] {
        let root = f.home.path().join(name);
        std::fs::create_dir_all(root.join(".git")).unwrap();
        std::fs::write(root.join(".git/HEAD"), "ref: refs/heads/main\n").unwrap();
        std::fs::write(
            root.join(".git/config"),
            format!("[remote \"origin\"]\nurl = https://example.invalid/{name}.git\n"),
        )
        .unwrap();
        std::fs::write(
            root.join(".tokenimum.toml"),
            format!("[loop]\nmode = \"{mode}\"\n"),
        )
        .unwrap();
        let key = tkm_config::repo::repo_identity(&root, "", Some(f.home.path())).key;
        store
            .upsert_repo(&tkm_store::Repo {
                repo_key: key.clone(),
                display_name: name.into(),
                first_seen: Timestamp(1),
                last_seen: Timestamp(1),
                delegated_runs: 0,
            })
            .unwrap();
        store
            .upsert_session(&NewSession {
                key: SessionKey {
                    host: Host::Claude,
                    agent_home: "/fixture".into(),
                    session_id: name.into(),
                },
                repo_key: key.clone(),
                cwd: root,
                started: Timestamp(1),
                ended: None,
                fold_status: "open".into(),
                shadow: false,
                integration_level: "hooks".into(),
            })
            .unwrap();
        keys.push((key, mode));
    }
    let out = f.run(&["status", "--all", "--json"]);
    assert!(out.status.success(), "{out:?}");
    let json: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let repos = json["repositories"].as_array().unwrap();
    for (key, mode) in keys {
        let report = repos.iter().find(|r| r["repo_key"] == key.0).unwrap();
        assert_eq!(report["loop_guard"]["mode"], mode);
        assert_eq!(report["config_source"], "global + repository");
    }
    let fallback = repos.iter().find(|r| r["repo_key"] == f.repo.0).unwrap();
    assert_eq!(fallback["config_source"], "global only (checkout unknown)");
    let out = f.run(&["status", "--all"]);
    assert!(String::from_utf8_lossy(&out.stdout).contains("global only (checkout unknown)"));
}
