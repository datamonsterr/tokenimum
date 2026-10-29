#![allow(clippy::unwrap_used, clippy::expect_used, clippy::pedantic)]
#![forbid(unsafe_code)]
#![cfg(target_os = "linux")]

mod support_init;
use std::{fs, path::PathBuf};
use support_init::{Fixture, stdout};
use tkm_core::model::{RepoKey, Timestamp};
use tkm_store::{NewSession, Repo, SessionKey, Store};

fn session(f: &Fixture, id: &str, repo: Option<RepoKey>, skills: &[&str]) -> PathBuf {
    let repo =
        repo.unwrap_or_else(|| tkm_config::repo::repo_identity(&f.home, "", Some(&f.home)).key);
    let store = Store::open(&f.paths.db).unwrap();
    store
        .upsert_repo(&Repo {
            repo_key: repo.clone(),
            display_name: "fixture".into(),
            first_seen: Timestamp(0),
            last_seen: Timestamp(0),
            delegated_runs: 0,
        })
        .unwrap();
    store
        .upsert_session(&NewSession {
            key: SessionKey {
                host: tkm_proto::Host::Claude,
                agent_home: f.home.join(".claude").to_string_lossy().into_owned(),
                session_id: id.into(),
            },
            repo_key: repo,
            cwd: f.home.clone(),
            started: Timestamp(1),
            ended: Some(Timestamp(2)),
            fold_status: "folded".into(),
            shadow: false,
            integration_level: "hooks".into(),
        })
        .unwrap();
    let slug: String = f
        .home
        .to_string_lossy()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    let logs = f.home.join(".claude/projects").join(slug);
    fs::create_dir_all(&logs).unwrap();
    let mut records =
        vec![serde_json::json!({"type":"user","message":{"content":"fixture prompt"}})];
    let calls: Vec<_> = skills.iter().enumerate().map(|(i, name)| serde_json::json!({"type":"tool_use", "id":format!("call-{i}"),"name":"Skill","input":{"skill":name}})).collect();
    records.push(serde_json::json!({"type":"assistant","message":{"content":calls}}));
    let text = records
        .iter()
        .map(|r| r.to_string() + "\n")
        .collect::<String>();
    let log = logs.join(format!("{id}.jsonl"));
    fs::write(&log, text).unwrap();
    log
}

#[test]
fn skills_json_without_database_is_empty_ranking_prior() {
    let f = Fixture::new();
    let out = f.run(&["skills", "--json"]);
    assert!(out.status.success(), "{out:?}");
    let json: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(
        json,
        serde_json::json!({"version":1,"source":"tokenimum","skills":[]})
    );
    assert!(!f.paths.db.exists());
}

#[test]
fn skills_json_ranks_recorded_invocations_in_current_repository() {
    let f = Fixture::new();
    session(&f, "one", None, &["zeta", "alpha", "alpha"]);
    session(&f, "two", None, &["alpha"]);
    session(
        &f,
        "other-repo",
        Some(RepoKey("fixture-other-repository".into())),
        &["foreign"],
    );
    let before = fs::read(&f.paths.db).unwrap();
    let out = f.run(&["skills", "--json"]);
    assert!(out.status.success(), "{out:?}");
    let json: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(
        json,
        serde_json::json!({"version":1,"source":"tokenimum","skills":[{"name":"alpha","loaded":0,"used":3},{"name":"zeta","loaded":0,"used":1}]})
    );
    assert!(out.stderr.is_empty(), "{out:?}");
    assert_eq!(fs::read(&f.paths.db).unwrap(), before);
}

#[test]
fn skills_text_prints_readable_evidence_table() {
    let f = Fixture::new();
    session(&f, "one", None, &["alpha", "alpha"]);
    let out = f.run(&["skills"]);
    assert!(out.status.success(), "{out:?}");
    let text = stdout(&out);
    for column in ["Skill", "Loaded", "Used", "alpha"] {
        assert!(text.contains(column), "{text}");
    }
    assert!(
        text.lines()
            .any(|line| line.contains("alpha") && line.split_whitespace().last() == Some("2")),
        "{text}"
    );
}

#[test]
fn skills_skips_missing_partial_and_oversize_logs_with_one_note() {
    let f = Fixture::new();
    let missing = session(&f, "missing", None, &["missing-skill"]);
    fs::remove_file(missing).unwrap();
    let partial = session(&f, "partial", None, &["partial-skill"]);
    fs::write(partial, "bad json\n").unwrap();
    let oversized = session(&f, "oversized", None, &["large-skill"]);
    fs::OpenOptions::new()
        .write(true)
        .open(oversized)
        .unwrap()
        .set_len(5 * 1024 * 1024)
        .unwrap();
    session(&f, "good", None, &["alpha"]);
    let out = f.run(&["skills", "--json"]);
    assert!(out.status.success(), "{out:?}");
    let json: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(json["skills"].as_array().unwrap().len(), 1);
    let error = String::from_utf8(out.stderr).unwrap();
    assert_eq!(error.lines().count(), 1, "{error}");
    assert!(error.contains("skipped 3"), "{error}");
}

#[test]
fn skills_limits_reconstruction_to_thirty_recent_sessions() {
    let f = Fixture::new();
    for index in 0..31 {
        session(&f, &format!("fixture-{index:02}"), None, &["alpha"]);
    }
    let out = f.run(&["skills", "--json"]);
    assert!(out.status.success(), "{out:?}");
    let json: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(json["skills"][0]["used"], 30);
}
