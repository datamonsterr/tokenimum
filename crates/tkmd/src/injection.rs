//! E4.4 inject layer (SPEC 7.2): candidates, relevance scoring, the 10-minute cache and the
//! cached default block used when scoring is late or unavailable.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use tkm_core::facts::Scope;
use tkm_core::inject::{self, InjectCandidate, Source};
use tkm_core::model::{Host, RepoKey, StopReason, Timestamp};
use tkm_decide::api::{Answer, Question};
use tkm_decide::cache::Purpose;
use tkm_decide::client::{DecideRequest, NoDecisionReason, Outcome};
use tkm_decide::redact::{RedactContext, Redactor};
use tkm_hosts::HostAdapter;
use tkm_proto::{Reply, ReplyAction, Request};
use tkm_store::FactState;

use crate::daemon::{Daemon, HResult, HandlerError, lock};
use crate::settings::Settings;
use crate::state::{StateMap, Tag};

/// Criteria of a relevance score question (SPEC 7.2 step 2).
const RELEVANCE_CRITERIA: [&str; 4] = [
    "irrelevant to this prompt",
    "might help",
    "likely needed",
    "required",
];

/// Everything the block builder needs about the prompt, copied out of the state lock.
#[derive(Clone)]
struct PromptContext {
    host: Host,
    session_id: String,
    repo: RepoKey,
    label: String,
    root: PathBuf,
    cwd: PathBuf,
    agent_home: PathBuf,
    prompt: String,
    settings: Arc<Settings>,
    injected: std::collections::BTreeSet<String>,
    evidence: Arc<std::sync::atomic::AtomicI64>,
}

#[derive(Default)]
struct BuiltBlock {
    text: String,
    ids: Vec<String>,
    fact_ids: Vec<String>,
}

fn remaining(budget: Duration, elapsed: Duration) -> Duration {
    budget.saturating_sub(elapsed)
}

fn fnv(text: &str) -> u64 {
    text.bytes().fold(0xcbf2_9ce4_8422_2325_u64, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x0000_0100_0000_01b3)
    })
}

/// Pointer lines (`- [title](file.md) — hook`) of one `MEMORY.md`.
fn memory_lines(path: &Path) -> Vec<String> {
    let Ok(text) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    text.lines()
        .filter_map(|line| line.trim().strip_prefix("- "))
        .map(|line| line.chars().take(300).collect::<String>())
        .filter(|line| !line.trim().is_empty())
        .take(20)
        .collect()
}

fn project_slug(cwd: &Path) -> String {
    cwd.to_string_lossy()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect()
}

impl Daemon {
    pub(crate) async fn prompt(
        self: &Arc<Self>,
        request: &Request,
        adapter: &dyn HostAdapter,
        event: tkm_core::model::HookEvent,
    ) -> HResult<Reply> {
        let seed = self.seed(request, &event);
        self.refresh_session_credentials(&seed);
        self.ensure_session(seed, false).await;
        let tag: Tag = (event.host, event.session_id.clone());
        let prompt = event.prompt.clone().unwrap_or_default();
        let now = self.clock.now_ms();

        let context = {
            let mut guard = lock(&self.state);
            let mut outbox = lock(&self.outbox);
            let had_calls = guard
                .sessions
                .get(&tag)
                .and_then(|s| s.turn.as_ref())
                .is_some_and(|t| t.total_calls > 0);
            if had_calls {
                // A prompt while a turn is open means the person interrupted it.
                self.finish_turn(&mut guard, &tag, StopReason::Interrupted, &mut outbox);
            }
            drop(outbox);
            let StateMap {
                sessions, turn_seq, ..
            } = &mut *guard;
            let Some(session) = sessions.get_mut(&tag) else {
                return Ok(crate::daemon::noop_reply(request.host));
            };
            session.last_activity_ms = now;
            *turn_seq += 1;
            self.begin_turn(session, *turn_seq, prompt.clone());
            PromptContext {
                host: session.host,
                session_id: session.session_id.clone(),
                repo: session.repo.clone(),
                label: session.repo_label.clone(),
                root: session.root.clone().unwrap_or_else(|| session.cwd.clone()),
                cwd: session.cwd.clone(),
                agent_home: session.agent_home.clone(),
                prompt,
                settings: session.settings.clone(),
                injected: session.injected.clone(),
                evidence: Arc::new(std::sync::atomic::AtomicI64::new(0)),
            }
        };

        let built = if context.settings.inject_enabled
            && self.live_allowed(&context.settings, context.host)
        {
            self.build_block(&context).await?
        } else {
            None
        };
        let BuiltBlock {
            text: block,
            ids,
            fact_ids,
        } = built.unwrap_or_default();
        let text = {
            let mut guard = lock(&self.state);
            let Some(session) = guard.sessions.get_mut(&tag) else {
                return Ok(crate::daemon::noop_reply(request.host));
            };
            session.injected.extend(ids.clone());
            let pending = session.pending_inject.take();
            let block = (!block.is_empty()).then_some(block);
            match (pending, block) {
                (Some(a), Some(b)) => Some(format!("{a}\n{b}")),
                (a, b) => a.or(b),
            }
        };
        let Some(text) = text else {
            return Ok(crate::daemon::noop_reply(request.host));
        };
        let (reply, delivered) =
            Self::render(request, adapter, ReplyAction::Inject { text: text.clone() });
        if delivered && !fact_ids.is_empty() {
            lock(&self.outbox).events.push_back(crate::persist::EventInsert {
                key: tkm_store::SessionKey {host:context.host,agent_home:context.agent_home.to_string_lossy().into_owned(),session_id:context.session_id.clone()},
                kind: "fact.injected".into(), inputs: serde_json::json!({"fact_ids":fact_ids}), template: "fact_injected".into(),
                params: serde_json::json!({"detail":text,"why":"selected repository facts passed lifecycle and relevance checks"}),
                created_at: Timestamp(i64::try_from(self.clock.now_ms()).unwrap_or(i64::MAX)), evidence_ids:Vec::new(), decision_id:Some(context.evidence.clone()),
            });
        }
        if !delivered && let Some(session) = lock(&self.state).sessions.get_mut(&tag) {
            // This event cannot inject (for example Cursor's beforeSubmitPrompt): keep the block
            // for the next event that can.
            session.pending_inject = Some(text);
        }
        Ok(reply)
    }

    /// Builds the block for a prompt; `None` when there are no new candidates.
    async fn build_block(self: &Arc<Self>, context: &PromptContext) -> HResult<Option<BuiltBlock>> {
        let started = Instant::now();
        let me = self.clone();
        let loading = context.clone();
        let candidates = tokio::task::spawn_blocking(move || me.load_candidates(&loading))
            .await
            .map_err(|error| HandlerError::Other(format!("candidate task: {error}")))??;
        let items: Vec<InjectCandidate> = candidates
            .into_iter()
            .filter(|item| !context.injected.contains(&item.id))
            .collect();
        if items.is_empty() {
            return Ok(None);
        }
        let config = context.settings.inject;
        let now = Timestamp(i64::try_from(self.clock.now_ms()).unwrap_or(i64::MAX));
        let key = inject::cache_key(&context.repo, &context.prompt, &items, config);
        let cached = lock(&self.inject_cache).get(key, now).map(str::to_owned);
        if let Some(block) = cached {
            let ids = Self::ids_in_block(&items, &block);
            let fact_ids = items
                .iter()
                .filter(|item| item.source == Source::Facts && ids.contains(&item.id))
                .map(|item| item.id.clone())
                .collect();
            return Ok(Some(BuiltBlock {
                text: block,
                ids,
                fact_ids,
            }));
        }
        let wait = remaining(
            Duration::from_millis(context.settings.relevance_timeout_ms),
            started.elapsed(),
        );
        let scores = self.score_candidates(context, &items, wait).await;
        let (selected, block) = if let Some(scores) = scores {
            let selected = inject::select(&items, Some(&scores), config);
            let block = inject::render(&selected);
            lock(&self.inject_cache).insert(key, now, block.clone());
            (selected, block)
        } else {
            let block = lock(&self.inject_cache).default_block(&items, config);
            (inject::select(&items, None, config), block)
        };
        let fact_ids = selected
            .iter()
            .filter(|item| item.source == Source::Facts)
            .map(|item| item.id.clone())
            .collect();
        Ok(Some(BuiltBlock {
            text: block,
            ids: selected.into_iter().map(|item| item.id).collect(),
            fact_ids,
        }))
    }

    fn ids_in_block(items: &[InjectCandidate], block: &str) -> Vec<String> {
        items
            .iter()
            .filter(|item| block.contains(item.text.trim()))
            .map(|item| item.id.clone())
            .collect()
    }

    /// Asks the decision client to score every candidate, within the prompt's read timeout minus
    /// a margin. `None` means "use the default block".
    async fn score_candidates(
        &self,
        context: &PromptContext,
        items: &[InjectCandidate],
        wait: Duration,
    ) -> Option<BTreeMap<String, f64>> {
        if !context.settings.decide.enabled || wait.is_zero() {
            return None;
        }
        let redactor = Redactor::new(RedactContext {
            repo_root: context.root.clone(),
            home: self
                .env
                .iter()
                .rev()
                .find(|(k, _)| k == "HOME")
                .map(|(_, v)| PathBuf::from(v))
                .unwrap_or_default(),
            env: self.env.clone(),
        });
        let mut state = format!(
            "prompt: {}\nrepository: {}\ncandidates:\n",
            redactor.redact(&context.prompt.chars().take(500).collect::<String>()),
            context.label
        );
        let mut questions = BTreeMap::new();
        for item in items {
            let _ = writeln!(state, "{}: {}", item.id, redactor.redact(&item.text));
            questions.insert(
                format!("rel_{}", item.id),
                Question::Score {
                    instructions: if item.source == Source::Memory {
                        let reference = item.text.split_once("](").and_then(|(_, tail)| tail.split_once(')')).map(|(path, _)| path);
                        reference.map_or_else(|| "How relevant is this candidate context to the prompt?".into(), |path|
                            format!("How relevant is this candidate context to the prompt?\nReference: {}", redactor.redact(path)))
                    } else { "How relevant is this candidate context to the prompt?".into() },
                    criteria: RELEVANCE_CRITERIA.iter().map(|c| (*c).to_owned()).collect(),
                },
            );
        }
        let request = DecideRequest {
            purpose: Purpose::Relevance,
            session_id: format!("{}:{}", context.host.as_str(), context.session_id),
            state: state.clone(),
            questions: questions.clone(),
        };
        match tokio::time::timeout(wait, self.decider.decide(request)).await {
            Ok(Outcome::Decision(decision)) => {
                let key = tkm_decide::calibrate::CalibrationKey {
                    model_version: decision.model_version.clone(),
                    purpose: Purpose::Relevance.as_str().into(),
                };
                let thresholds =
                    tkm_decide::calibrate::CalibrationStore::get(self.calibration.as_ref(), &key);
                let backing = self.backing.clone();
                let now = i64::try_from(self.clock.now_ms()).unwrap_or(i64::MAX);
                let days = context.settings.shadow_days;
                let turns = context.settings.shadow_turns;
                let shadow = tokio::task::spawn_blocking(move || {
                    backing.open().map_or(true, |store| {
                        crate::decider::purpose_shadow(&store, "relevance", now, days, turns)
                    })
                })
                .await
                .unwrap_or(true);
                let acting = thresholds.is_some() && !shadow;
                let mut scores = BTreeMap::new();
                for item in items {
                    if let Some(Answer::Score { score, .. }) =
                        decision.answers.get(&format!("rel_{}", item.id))
                    {
                        let thresholds = thresholds.unwrap_or_default();
                        let cutoff = if item.fire_gate_prevents {
                            thresholds.t_low
                        } else {
                            thresholds.t_high
                        } * 3.0;
                        scores.insert(item.id.clone(), if *score >= cutoff { *score } else { 0.0 });
                    }
                }
                lock(&self.outbox)
                    .decisions
                    .push_back(crate::persist::DecisionInsert {
                        evidence_id: context.evidence.clone(),
                        created_at: Timestamp(
                            i64::try_from(self.clock.now_ms()).unwrap_or(i64::MAX),
                        ),
                        repo: Some(context.repo.clone()),
                        key: Some(tkm_store::SessionKey {
                            host: context.host,
                            agent_home: context.agent_home.to_string_lossy().into_owned(),
                            session_id: context.session_id.clone(),
                        }),
                        digest: Some(state),
                        decision: tkm_store::NewDecision {
                            model: decision.model_version.clone(),
                            purpose: Purpose::Relevance.as_str().to_owned(),
                            questions: serde_json::to_value(&questions).unwrap_or_default(),
                            answers: serde_json::to_value(&decision.answers).unwrap_or_default(),
                            cost_usd: if decision.cached {
                                Some(0.0)
                            } else {
                                decision.usage.cost
                            },
                            latency_ms: u32::try_from(decision.latency_ms).unwrap_or(u32::MAX),
                            shadow: !acting,
                        },
                    });
                acting.then_some(scores)
            }
            Ok(Outcome::NoDecision(NoDecisionReason::Disabled) | Outcome::CodeOnly(_)) => None,
            Ok(Outcome::NoDecision(reason)) => {
                self.log_error(&format!(
                    "relevance: no decision ({reason:?}); using the default block"
                ));
                None
            }
            Err(_) => {
                self.log_error("relevance: scoring timed out; using the default block");
                None
            }
        }
    }

    /// Facts, memory pointer lines and skill candidates for the prompt. Runs on a blocking task.
    fn load_candidates(&self, context: &PromptContext) -> HResult<Vec<InjectCandidate>> {
        let mut items = Vec::new();
        let store = self.backing.open()?;
        for record in store.list_facts(&context.repo, Some(FactState::Active))? {
            let Some(fact) = crate::fold::fact_from_record(&record) else {
                continue;
            };
            let for_host = match fact.candidate.scope {
                Scope::Neutral => true,
                Scope::Host(host) => host == context.host,
            };
            if !context.settings.facts_enabled
                || !for_host
                || !fact.life.injectable(&context.settings.facts)
            {
                continue;
            }
            let text = tkm_core::facts::render::claim(&fact.candidate);
            if text.is_empty() {
                continue;
            }
            items.push(InjectCandidate {
                id: record.fact_id,
                source: Source::Facts,
                tokens: inject::estimate_tokens(&text),
                text,
                fire_gate_prevents: true,
            });
        }
        if context.settings.inject.memory {
            let mut paths = vec![
                context.root.join("MEMORY.md"),
                context.cwd.join("MEMORY.md"),
                context.agent_home.join("MEMORY.md"),
                context
                    .agent_home
                    .join("projects")
                    .join(project_slug(&context.cwd))
                    .join("memory")
                    .join("MEMORY.md"),
            ];
            paths.dedup();
            let mut seen = std::collections::BTreeSet::new();
            for path in paths {
                for line in memory_lines(&path) {
                    if seen.insert(line.clone()) {
                        items.push(InjectCandidate {
                            id: format!("memory:{:016x}", fnv(&line)),
                            source: Source::Memory,
                            tokens: inject::estimate_tokens(&line),
                            text: line,
                            fire_gate_prevents: false,
                        });
                    }
                }
            }
        }
        if context.settings.otherskill_mode != tkm_integrations::detect::Mode::Off {
            for skill in self.skills.candidates_in(
                &context.repo,
                context.host,
                &context.prompt,
                &context.cwd,
            ) {
                items.push(InjectCandidate {
                    id: format!("skill:{}", skill.id),
                    source: Source::Skills,
                    tokens: inject::estimate_tokens(&skill.text),
                    text: skill.text,
                    fire_gate_prevents: false,
                });
            }
        }
        Ok(items)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scoring_budget_subtracts_candidate_loading() {
        assert_eq!(
            remaining(Duration::from_millis(1200), Duration::from_millis(700)),
            Duration::from_millis(500)
        );
        assert_eq!(
            remaining(Duration::from_millis(1200), Duration::from_millis(1500)),
            Duration::ZERO
        );
    }

    #[test]
    fn memory_lines_keep_only_pointer_bullets() {
        let dir = std::env::temp_dir().join(format!("tkmd-mem-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("MEMORY.md");
        std::fs::write(
            &file,
            "# Memory\n\n- [A](a.md) — first\nnot a bullet\n- [B](b.md) — second\n",
        )
        .unwrap();
        assert_eq!(
            memory_lines(&file),
            vec!["[A](a.md) — first", "[B](b.md) — second"]
        );
        assert_eq!(memory_lines(&dir.join("missing.md")).len(), 0);
    }

    #[test]
    fn project_slug_replaces_separators() {
        assert_eq!(project_slug(Path::new("/home/u/dev/x")), "-home-u-dev-x");
    }
}
