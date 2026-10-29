//! Deterministic, offline replay of synthetic decision responses.
use crate::error::CliError;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt::Write as _,
    path::Path,
};
use tkm_core::model::{Host, RepoKey, StopReason};
use tkm_decide::{
    api::{self, Answer, DecisionsResponse},
    calibrate::{FitConfig, fit_upper},
    digest::{DigestInput, build_digest},
    redact::{RedactContext, Redactor},
};
use tkm_hosts::{HostAdapter, SessionSource};

#[derive(Serialize, Deserialize)]
struct CacheFile {
    source: String,
    entries: Vec<Cached>,
}

#[derive(Serialize, Deserialize)]
struct Cached {
    session: String,
    turn: usize,
    latency_ms: u64,
    fixture_sha256: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    digest: Option<String>,
    questions: BTreeMap<String, tkm_decide::api::Question>,
    response: DecisionsResponse,
    fact_candidate: String,
    fact_questions: BTreeMap<String, tkm_decide::api::Question>,
    fact_latency_ms: u64,
    fact_response: DecisionsResponse,
}

struct BenchTurn {
    session: String,
    turn: usize,
    digest: String,
    fixture_sha256: String,
    fact_candidate: String,
}

pub(super) struct Bench {
    source: String,
    sessions: usize,
    claude: usize,
    codex: usize,
    pub(super) samples: BTreeMap<String, Vec<(f64, bool)>>,
    totals: BTreeMap<String, (u64, f64, usize)>,
    fact_totals: BTreeMap<String, (u64, f64, usize, usize)>,
}

#[allow(clippy::too_many_lines)] // Fixture normalization, cache validation and aggregation form one pass.
pub(super) fn load(root: &Path) -> Result<Bench, CliError> {
    let file: CacheFile = serde_json::from_slice(&std::fs::read(root.join("cache.json"))?)
        .map_err(|e| CliError::Usage(format!("invalid bench cache: {e}")))?;
    if file.source != "synthetic test double"
        && file.source != "recorded OpenRouter response"
        && file.source != "recorded test endpoint response"
    {
        return Err(CliError::Usage("unknown bench cache provenance".into()));
    }
    let cache = file.entries;
    let mut paths = std::fs::read_dir(root.join("sessions"))?
        .map(|entry| entry.map(|e| e.path()))
        .collect::<Result<Vec<_>, _>>()?;
    paths.sort();
    let mut bench = Bench {
        source: file.source,
        sessions: 0,
        claude: 0,
        codex: 0,
        samples: BTreeMap::new(),
        totals: BTreeMap::new(),
        fact_totals: BTreeMap::new(),
    };
    let mut used = BTreeSet::new();
    let models: BTreeSet<_> = cache.iter().map(|row| row.response.model.clone()).collect();
    if models.is_empty() {
        return Err(CliError::Usage("empty bench cache".into()));
    }
    let redactor = Redactor::new(RedactContext {
        repo_root: "/synthetic/repo".into(),
        home: "/synthetic/home".into(),
        env: Vec::new(),
    });
    let questions = tkmd::live::judge_questions();
    for path in paths {
        if path.extension().and_then(|s| s.to_str()) != Some("jsonl") {
            continue;
        }
        let id = path
            .file_stem()
            .and_then(|s| s.to_str())
            .ok_or_else(|| CliError::Usage("invalid fixture name".into()))?
            .to_owned();
        let (host, adapter): (Host, Box<dyn HostAdapter>) = if id.starts_with("claude-") {
            (
                Host::Claude,
                Box::new(tkm_hosts::claude::ClaudeAdapter::new()),
            )
        } else if id.starts_with("codex-") {
            (Host::Codex, Box::new(tkm_hosts::codex::CodexAdapter::new()))
        } else {
            return Err(CliError::Usage(format!("unknown fixture host: {id}")));
        };
        let fixture_hash = format!("{:x}", Sha256::digest(std::fs::read(&path)?));
        let source = SessionSource {
            path,
            session_id: id.clone(),
            cwd: "/synthetic/repo".into(),
            repo_key: RepoKey("synthetic".into()),
            ..SessionSource::default()
        };
        let session = adapter
            .read_session(&source)
            .map_err(|e| CliError::Usage(format!("invalid fixture {id}: {e}")))?;
        if session.partial || session.turns.is_empty() {
            return Err(CliError::Usage(format!("incomplete fixture {id}")));
        }
        bench.sessions += 1;
        if host == Host::Claude {
            bench.claude += 1;
        } else {
            bench.codex += 1;
        }
        for (turn_index, turn) in session.turns.iter().enumerate() {
            let label = match turn.stopped_by {
                StopReason::Interrupted => true,
                StopReason::OwnStop => false,
                _ => {
                    return Err(CliError::Usage(format!(
                        "fixture {id} turn {turn_index} has no outcome"
                    )));
                }
            };
            let digest = build_digest(
                DigestInput {
                    prompt: &turn.prompt,
                    calls: &turn.calls,
                    tests: None,
                },
                &redactor,
            );
            if digest.is_empty() {
                return Err(CliError::Usage(format!("empty digest in {id}")));
            }
            let candidate = turn
                .calls
                .first()
                .map(|call| redactor.redact(&call.args_digest))
                .filter(|text| !text.is_empty())
                .ok_or_else(|| CliError::Usage(format!("fixture {id} has no fact candidate")))?;
            if !matches!(
                candidate.as_str(),
                "cargo test sample"
                    | "rg pattern src"
                    | "cat example.txt"
                    | "cargo fmt --check"
                    | "git diff"
            ) {
                return Err(CliError::Usage(format!(
                    "fixture {id} has no fact label contract"
                )));
            }
            for model in &models {
                let matches: Vec<_> = cache
                    .iter()
                    .enumerate()
                    .filter(|(_, row)| {
                        row.session == id && row.turn == turn_index && &row.response.model == model
                    })
                    .collect();
                let [(index, row)] = matches.as_slice() else {
                    return Err(CliError::Usage(format!(
                        "missing or duplicate cached response for {id}/{turn_index}/{model}"
                    )));
                };
                if row.fixture_sha256 != fixture_hash
                    || row.questions != questions
                    || (bench.source != "synthetic test double"
                        && row.digest.as_deref() != Some(digest.as_str()))
                {
                    return Err(CliError::Usage(format!(
                        "stale cached request for {id}/{turn_index}/{model}"
                    )));
                }
                api::validate(&questions, &row.response)
                    .map_err(|e| CliError::Usage(format!("invalid cached response: {e}")))?;
                let Some(Answer::Noul { noul }) = row.response.answers.get("stuck") else {
                    return Err(CliError::Usage("missing stuck answer".into()));
                };
                let cost = row
                    .response
                    .usage
                    .cost
                    .ok_or_else(|| CliError::Usage("missing cached cost".into()))?;
                if !cost.is_finite() || cost < 0.0 {
                    return Err(CliError::Usage("invalid cached cost".into()));
                }
                let fact_questions = fact_questions();
                if row.fact_candidate != candidate
                    || row.fact_questions != fact_questions
                    || row.fact_response.model != row.response.model
                {
                    return Err(CliError::Usage(format!(
                        "stale fact cache for {id}/{turn_index}/{model}"
                    )));
                }
                api::validate(&fact_questions, &row.fact_response)
                    .map_err(|e| CliError::Usage(format!("invalid cached fact response: {e}")))?;
                let Some(Answer::Noul { noul: stable }) =
                    row.fact_response.answers.get("stable_candidate")
                else {
                    return Err(CliError::Usage("missing stable answer".into()));
                };
                let Some(Answer::Noul { noul: safe }) =
                    row.fact_response.answers.get("safe_candidate")
                else {
                    return Err(CliError::Usage("missing safe answer".into()));
                };
                let fact_cost = row
                    .fact_response
                    .usage
                    .cost
                    .filter(|cost| cost.is_finite() && *cost >= 0.0)
                    .ok_or_else(|| CliError::Usage("invalid cached fact cost".into()))?;
                let fact_total = bench.fact_totals.entry(model.clone()).or_default();
                fact_total.0 += row.fact_latency_ms;
                fact_total.1 += fact_cost;
                fact_total.2 += usize::from(*stable < 0.30);
                fact_total.3 += usize::from(*safe >= 0.80);
                used.insert(*index);
                bench
                    .samples
                    .entry(model.clone())
                    .or_default()
                    .push((*noul, label));
                let total = bench.totals.entry(model.clone()).or_default();
                total.0 += row.latency_ms;
                total.1 += cost;
                total.2 += usize::from((*noul >= 0.85) == label);
            }
        }
    }
    if bench.sessions < 30 || used.len() != cache.len() {
        return Err(CliError::Usage(
            "bench requires at least 30 sessions and no unused cache entries".into(),
        ));
    }
    Ok(bench)
}

impl Bench {
    pub(super) fn render(&self) -> String {
        let mut output = format!(
            "# M0 decision bench\n\nsource: {}. Real per-model numbers need a recorded cache; run `tkm replay --bench --record fixtures/bench` with OPENROUTER_API_KEY set. Synthetic test doubles are not real model measurements.\n\n{} sessions: {} Claude Code, {} Codex. Labels derive from interruption (stuck) or normal completion (progress). Each turn uses the production loop-guard questions and validated cached responses.\n\n| Model | Turns | Agreement at 0.85 | Mean latency (ms) | Total cost (USD) | Fitted stuck threshold | precision | Recall |\n|---|---:|---:|---:|---:|---:|---:|---:|\n",
            self.source, self.sessions, self.claude, self.codex
        );
        for (model, samples) in &self.samples {
            let (latency, cost, agreement) = self.totals[model];
            let fit = fit_upper(samples, &FitConfig::default());
            let (threshold, precision, recall) = fit.map_or_else(
                || ("—".to_owned(), "—".to_owned(), "—".to_owned()),
                |fit| {
                    (
                        format!("{:.2}", fit.threshold),
                        format!("{:.3}", fit.precision),
                        format!("{:.3}", fit.recall),
                    )
                },
            );
            let _ = writeln!(
                output,
                "| {model} | {} | {agreement}/{} | {} | ${cost:.6} | {threshold} | {precision} | {recall} |",
                samples.len(),
                samples.len(),
                latency / u64::try_from(samples.len()).unwrap_or(1)
            );
        }
        output.push_str("\nFact filter: the first command is a task-specific, harmless candidate in each synthetic turn. Agreement is against those fixture labels using stable < 0.30 and safe >= 0.80.\n\n| Model | stable agreement | safe agreement | Mean latency (ms) | Total cost (USD) |\n|---|---:|---:|---:|---:|\n");
        for (model, (latency, cost, stable, safe)) in &self.fact_totals {
            let count = self.samples[model].len();
            let _ = writeln!(
                output,
                "| {model} | {stable}/{count} | {safe}/{count} | {} | ${cost:.6} |",
                latency / u64::try_from(count).unwrap_or(1)
            );
        }
        output
    }
}

fn fact_questions() -> BTreeMap<String, tkm_decide::api::Question> {
    use tkm_decide::api::{NoulCriteria, Question};
    BTreeMap::from([
        (
            "stable_candidate".into(),
            Question::Noul {
                instructions: "Is the candidate stable for this repository regardless of task?"
                    .into(),
                criteria: Some(NoulCriteria {
                    when_true: "holds for this repository regardless of task".into(),
                    when_false: "specific to this task".into(),
                }),
            },
        ),
        (
            "safe_candidate".into(),
            Question::Noul {
                instructions: "Is the candidate harmless and safe to reuse?".into(),
                criteria: Some(NoulCriteria {
                    when_true: "a harmless command or setting".into(),
                    when_false: "destructive, secret-bearing, or completion advice".into(),
                }),
            },
        ),
    ])
}

/// Refresh the whole cache only after every request succeeds and validates.
#[allow(clippy::too_many_lines)] // Each model is recorded for both decision purposes atomically.
pub(super) fn record(ctx: &crate::ctx::Ctx<'_>, root: &Path) -> Result<(), CliError> {
    use tkm_decide::{
        api::DecisionsRequest,
        client::{HttpTransport, Transport},
        key::load_key,
    };
    let inputs = recording_inputs(root, &ctx.env)?;
    let get_env = |name: &str| {
        ctx.env
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.as_str())
    };
    let secret = get_env("OPENROUTER_API_KEY")
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| CliError::Usage("--record requires OPENROUTER_API_KEY".into()))?;
    let key = load_key(Some(secret.to_owned()), Path::new(""), None)
        .map_err(|_| CliError::Usage("invalid decision key".into()))?;
    let config = tkmd::settings::Settings::from_config(&ctx.load_config()?).decide;
    if !config.enabled {
        return Err(CliError::Usage(
            "decision recording is disabled in configuration".into(),
        ));
    }
    config
        .validate()
        .map_err(|e| CliError::Usage(format!("invalid decision model configuration: {e}")))?;
    let endpoint = get_env("TKM_DECIDE_ENDPOINT");
    let transport = match endpoint {
        Some(url) => HttpTransport::with_endpoint(url),
        None => HttpTransport::new(),
    }
    .map_err(|e| CliError::Usage(format!("cannot create decision transport: {e}")))?;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    let questions = tkmd::live::judge_questions();
    let mut entries = Vec::new();
    let mut total_cost = 0.0;
    let mut session_costs = BTreeMap::<String, f64>::new();
    let mut model_versions = BTreeMap::<String, String>::new();
    for turn in &inputs {
        let session_cost = session_costs.entry(turn.session.clone()).or_default();
        for model in config.chain.iter().filter(|model| !model.free_probe) {
            if total_cost >= config.daily_cap_usd || *session_cost >= config.session_cap_usd {
                return Err(CliError::Usage(
                    "recording decision budget exhausted".into(),
                ));
            }
            let request = DecisionsRequest {
                model: model.slug.clone(),
                state: serde_json::Value::String(turn.digest.clone()),
                questions: questions.clone(),
                provider: Some(config.provider.clone()),
                session_id: Some(turn.session.clone()),
                user: None,
            };
            let body = serde_json::to_string(&request)
                .map_err(|e| CliError::Usage(format!("cannot encode decision request: {e}")))?;
            let start = std::time::Instant::now();
            let reply = runtime
                .block_on(async {
                    tokio::time::timeout(
                        std::time::Duration::from_millis(config.timeout_ms),
                        transport.post_decision(&key, body),
                    )
                    .await
                })
                .map_err(|_| CliError::Usage("decision recording timed out".into()))?
                .map_err(|e| CliError::Usage(format!("decision recording failed: {e}")))?;
            let latency_ms = u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX);
            if !(200..300).contains(&reply.status) {
                return Err(CliError::Usage(format!(
                    "decision recording returned HTTP {}",
                    reply.status
                )));
            }
            let response: DecisionsResponse = serde_json::from_str(&reply.body)
                .map_err(|_| CliError::Usage("invalid recorded decision response".into()))?;
            api::validate(&questions, &response)
                .map_err(|e| CliError::Usage(format!("invalid recorded decision response: {e}")))?;
            let cost = response
                .usage
                .cost
                .filter(|cost| cost.is_finite() && *cost >= 0.0)
                .ok_or_else(|| CliError::Usage("recorded response lacks a valid cost".into()))?;
            total_cost += cost;
            *session_cost += cost;
            if let Some(version) = model_versions.get(&model.slug) {
                if version != &response.model {
                    return Err(CliError::Usage(
                        "recording model version changed during run".into(),
                    ));
                }
            } else {
                if model_versions
                    .values()
                    .any(|version| version == &response.model)
                {
                    return Err(CliError::Usage(
                        "two requested models returned the same version".into(),
                    ));
                }
                model_versions.insert(model.slug.clone(), response.model.clone());
            }
            if total_cost >= config.daily_cap_usd || *session_cost >= config.session_cap_usd {
                return Err(CliError::Usage(
                    "recording decision budget exhausted".into(),
                ));
            }
            let fact_questions = fact_questions();
            let fact_state = format!("{}\ncandidate: {}", turn.digest, turn.fact_candidate);
            if fact_state.len() > config.max_state_bytes {
                return Err(CliError::Usage(
                    "fact digest exceeds configured limit".into(),
                ));
            }
            let fact_request = DecisionsRequest {
                model: model.slug.clone(),
                state: serde_json::Value::String(fact_state),
                questions: fact_questions.clone(),
                provider: Some(config.provider.clone()),
                session_id: Some(turn.session.clone()),
                user: None,
            };
            let fact_body = serde_json::to_string(&fact_request)
                .map_err(|e| CliError::Usage(format!("cannot encode fact request: {e}")))?;
            let fact_start = std::time::Instant::now();
            let fact_reply = runtime
                .block_on(async {
                    tokio::time::timeout(
                        std::time::Duration::from_millis(config.timeout_ms),
                        transport.post_decision(&key, fact_body),
                    )
                    .await
                })
                .map_err(|_| CliError::Usage("fact recording timed out".into()))?
                .map_err(|e| CliError::Usage(format!("fact recording failed: {e}")))?;
            let fact_latency_ms =
                u64::try_from(fact_start.elapsed().as_millis()).unwrap_or(u64::MAX);
            if !(200..300).contains(&fact_reply.status) {
                return Err(CliError::Usage(format!(
                    "fact recording returned HTTP {}",
                    fact_reply.status
                )));
            }
            let fact_response: DecisionsResponse = serde_json::from_str(&fact_reply.body)
                .map_err(|_| CliError::Usage("invalid recorded fact response".into()))?;
            api::validate(&fact_questions, &fact_response)
                .map_err(|e| CliError::Usage(format!("invalid recorded fact response: {e}")))?;
            if fact_response.model != response.model {
                return Err(CliError::Usage(
                    "model changed between purpose requests".into(),
                ));
            }
            let fact_cost = fact_response
                .usage
                .cost
                .filter(|cost| cost.is_finite() && *cost >= 0.0)
                .ok_or_else(|| {
                    CliError::Usage("recorded fact response lacks a valid cost".into())
                })?;
            total_cost += fact_cost;
            *session_cost += fact_cost;
            entries.push(Cached {
                session: turn.session.clone(),
                turn: turn.turn,
                latency_ms,
                fixture_sha256: turn.fixture_sha256.clone(),
                digest: Some(turn.digest.clone()),
                questions: questions.clone(),
                response,
                fact_candidate: turn.fact_candidate.clone(),
                fact_questions,
                fact_latency_ms,
                fact_response,
            });
        }
    }
    if entries.is_empty() {
        return Err(CliError::Usage("no enabled recording models".into()));
    }
    let file = CacheFile {
        source: if endpoint.is_some() {
            "recorded test endpoint response"
        } else {
            "recorded OpenRouter response"
        }
        .into(),
        entries,
    };
    let mut bytes = serde_json::to_vec_pretty(&file)
        .map_err(|e| CliError::Usage(format!("cannot encode decision cache: {e}")))?;
    bytes.push(b'\n');
    let temp = root.join("cache.json.tmp");
    std::fs::write(&temp, bytes)?;
    std::fs::rename(temp, root.join("cache.json"))?;
    Ok(())
}

#[allow(clippy::too_many_lines)] // Every fixture must be normalized before sending any requests.
fn recording_inputs(root: &Path, env: &[(String, String)]) -> Result<Vec<BenchTurn>, CliError> {
    let mut paths = std::fs::read_dir(root.join("sessions"))?
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<Result<Vec<_>, _>>()?;
    paths.sort();
    let redactor = Redactor::new(RedactContext {
        repo_root: "/synthetic/repo".into(),
        home: env.iter().find(|(name, _)| name == "HOME").map_or_else(
            || "/synthetic/home".into(),
            |(_, value)| std::path::PathBuf::from(value.as_str()),
        ),
        env: env.to_vec(),
    });
    let mut result = Vec::new();
    let mut sessions = 0;
    for path in paths {
        if path.extension().and_then(|extension| extension.to_str()) != Some("jsonl") {
            continue;
        }
        let id = path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .ok_or_else(|| CliError::Usage("invalid fixture name".into()))?
            .to_owned();
        let adapter: Box<dyn HostAdapter> = if id.starts_with("claude-") {
            Box::new(tkm_hosts::claude::ClaudeAdapter::new())
        } else if id.starts_with("codex-") {
            Box::new(tkm_hosts::codex::CodexAdapter::new())
        } else {
            return Err(CliError::Usage(format!("unknown fixture host: {id}")));
        };
        let fixture_sha256 = format!("{:x}", Sha256::digest(std::fs::read(&path)?));
        let source = SessionSource {
            path,
            session_id: id.clone(),
            cwd: "/synthetic/repo".into(),
            repo_key: RepoKey("synthetic".into()),
            ..SessionSource::default()
        };
        let session = adapter
            .read_session(&source)
            .map_err(|e| CliError::Usage(format!("invalid fixture {id}: {e}")))?;
        if session.partial || session.turns.is_empty() {
            return Err(CliError::Usage(format!("incomplete fixture {id}")));
        }
        sessions += 1;
        for (turn, item) in session.turns.iter().enumerate() {
            if !matches!(
                item.stopped_by,
                StopReason::Interrupted | StopReason::OwnStop
            ) {
                return Err(CliError::Usage(format!("fixture {id} has no outcome")));
            }
            let fact_candidate = item
                .calls
                .first()
                .map(|call| redactor.redact(&call.args_digest))
                .filter(|candidate| !candidate.is_empty())
                .ok_or_else(|| CliError::Usage(format!("fixture {id} has no fact candidate")))?;
            if !matches!(
                fact_candidate.as_str(),
                "cargo test sample"
                    | "rg pattern src"
                    | "cat example.txt"
                    | "cargo fmt --check"
                    | "git diff"
            ) {
                return Err(CliError::Usage(format!(
                    "fixture {id} has no fact label contract"
                )));
            }
            let digest = build_digest(
                DigestInput {
                    prompt: &item.prompt,
                    calls: &item.calls,
                    tests: None,
                },
                &redactor,
            );
            result.push(BenchTurn {
                session: id.clone(),
                turn,
                digest,
                fixture_sha256: fixture_sha256.clone(),
                fact_candidate,
            });
        }
    }
    if sessions < 30 {
        return Err(CliError::Usage(
            "bench requires at least 30 sessions".into(),
        ));
    }
    Ok(result)
}
