// Portions ported from microsoft/TokenEconomizer (tokeneconomizer/microskills/envfacts.py, commit d0675a3),
// Copyright (c) Microsoft Corporation, licensed under the Apache License, Version 2.0.
//! Five code-only candidate detectors.

use super::observe::{self, Intent, Observation, Observations, Outcome};
use super::{Candidate, Evidence, Fact, Family, LabelRule, Params, Scope};
use std::collections::{BTreeMap, BTreeSet};

fn intent_name(intent: Intent) -> &'static str {
    match intent {
        Intent::RunTests => "run_tests",
        Intent::RunInterpreter => "run_interpreter",
        Intent::RunBuild => "run_build",
        Intent::PackageTool => "package_tool",
        Intent::FetchRemote => "fetch_remote",
        Intent::ProbeVcsRefs => "probe_vcs_refs",
        Intent::ProbeEnv => "probe_env",
    }
}
fn candidate(
    input: &Observations,
    id: String,
    family: Family,
    params: Params,
    calls: Vec<usize>,
) -> Candidate {
    let mut calls = calls;
    calls.sort_unstable();
    calls.dedup();
    Candidate {
        id,
        family,
        scope: scope_for(&params, input.host, &input.tool_names),
        params,
        evidence: vec![Evidence {
            session_id: input.session_id.clone(),
            call_indices: calls,
        }],
    }
}
/// Tags parameters mentioning a host catalog's tool names as host-specific (SPEC 7.1).
#[must_use]
pub fn scope_for(params: &Params, host: crate::model::Host, tool_names: &[String]) -> Scope {
    let forms = std::iter::once(params.bad_form.as_str())
        .chain(std::iter::once(params.form.as_str()))
        .chain(params.forms.keys().map(String::as_str));
    if forms
        .flat_map(str::split_whitespace)
        .any(|word| tool_names.iter().any(|tool| word == tool))
    {
        Scope::Host(host)
    } else {
        Scope::Neutral
    }
}
fn typed_ok(o: &Observation) -> bool {
    observe::form_worked(o) && !o.assigned
}
fn repairs(form: &str, bad: &str, intent: Intent) -> bool {
    form != bad
        && (matches!(intent, Intent::RunTests | Intent::RunBuild) || !observe::is_bare(form))
}
fn add(map: &mut BTreeMap<String, u32>, key: &str) {
    let count = map.entry(key.into()).or_default();
    *count = count.saturating_add(1);
}
fn discover_forms(input: &Observations, out: &mut Vec<Candidate>) {
    for a in &input.observations {
        if !matches!(
            a.intent,
            Intent::RunTests | Intent::RunBuild | Intent::RunInterpreter | Intent::PackageTool
        ) || !observe::form_failed(a)
            || (!matches!(a.intent, Intent::RunTests | Intent::RunBuild)
                && !observe::is_bare(&a.form))
        {
            continue;
        }
        let later: Vec<_> = input
            .observations
            .iter()
            .filter(|b| {
                b.intent == a.intent
                    && b.call > a.call
                    && typed_ok(b)
                    && repairs(&b.form, &a.form, a.intent)
                    && super::lint::command(&b.form)
            })
            .collect();
        if later.is_empty() {
            continue;
        }
        let mut params = Params {
            intent: Some(a.intent),
            bad_form: a.form.clone(),
            forms: input
                .observations
                .iter()
                .filter(|b| {
                    b.intent == a.intent
                        && typed_ok(b)
                        && repairs(&b.form, &a.form, a.intent)
                        && super::lint::command(&b.form)
                })
                .map(|b| (b.form.clone(), 1))
                .collect(),
            ..Params::default()
        };
        if let Outcome::Error(error) = a.outcome {
            params.errors.insert(error, 1);
        }
        let calls = std::iter::once(a.call)
            .chain(later.iter().map(|b| b.call))
            .collect();
        out.push(candidate(
            input,
            format!("form_repair:{}:{}", intent_name(a.intent), a.form),
            Family::FormRepair,
            params,
            calls,
        ));
    }
}
fn discover_args(input: &Observations, out: &mut Vec<Candidate>) {
    let runs: Vec<_> = input
        .observations
        .iter()
        .filter(|o| o.intent == Intent::RunTests && !o.labels.is_empty())
        .collect();
    for (i, a) in runs.iter().enumerate().filter(|(_, a)| a.label_error) {
        for bad in &a.labels {
            for b in &runs[i + 1..] {
                if b.form != a.form || !b.loaded {
                    continue;
                }
                let Some((good, rule)) = b
                    .labels
                    .iter()
                    .find_map(|good| observe::label_rule(bad, good).map(|rule| (good, rule)))
                else {
                    continue;
                };
                let key = match &rule {
                    LabelRule::Separator => "sep:::".into(),
                    LabelRule::Strip(p) => format!("strip:{p}"),
                };
                let params = Params {
                    form: a.form.clone(),
                    rule: Some(rule),
                    examples: BTreeMap::from([(bad.clone(), good.clone())]),
                    ..Params::default()
                };
                out.push(candidate(
                    input,
                    format!("arg_repair:{}:{key}", a.form),
                    Family::ArgumentRepair,
                    params,
                    vec![a.call, b.call],
                ));
                break;
            }
        }
    }
}
fn discover_env(input: &Observations, out: &mut Vec<Candidate>) {
    for (i, c) in input.env.iter().enumerate() {
        for var in &c.needed {
            let later: Vec<_> = input.env[i + 1..]
                .iter()
                .filter(|d| d.sets.contains_key(var))
                .collect();
            let Some(good) = later.iter().find(|d| d.ok && !d.needed.contains(var)) else {
                continue;
            };
            let Some(value) = good.sets.get(var) else {
                continue;
            };
            let mut params = Params {
                variable: var.clone(),
                good: BTreeMap::from([(value.clone(), 1)]),
                ..Params::default()
            };
            for d in &later {
                if !d.ok
                    && let Some(bad) = d.sets.get(var).filter(|bad| *bad != value)
                {
                    add(&mut params.bad, bad);
                }
            }
            out.push(candidate(
                input,
                format!("env_value:{var}"),
                Family::EnvValue,
                params,
                std::iter::once(c.call)
                    .chain(later.iter().map(|d| d.call))
                    .collect(),
            ));
        }
    }
}
/// Extracts regex-validated candidates with source references. Partial sessions abstain.
#[must_use]
pub fn extract(input: &Observations) -> Vec<Candidate> {
    if input.partial {
        return Vec::new();
    }
    let mut out = Vec::new();
    discover_forms(input, &mut out);
    discover_args(input, &mut out);
    discover_env(input, &mut out);
    let mut hosts = BTreeMap::new();
    let mut fetch_calls = Vec::new();
    for o in &input.observations {
        if o.intent == Intent::FetchRemote
            && matches!(o.outcome,Outcome::Error(e) if observe::network(e))
        {
            for host in &o.hosts {
                add(&mut hosts, host);
            }
            fetch_calls.push(o.call);
        }
    }
    if !hosts.is_empty() {
        out.push(candidate(
            input,
            "boundary:fetch_remote".into(),
            Family::Boundary,
            Params {
                hosts,
                ..Params::default()
            },
            fetch_calls,
        ));
    }
    let probes: Vec<_> = input
        .observations
        .iter()
        .filter(|o| {
            o.intent == Intent::ProbeVcsRefs && matches!(o.outcome, Outcome::Hit | Outcome::Empty)
        })
        .collect();
    if !probes.is_empty() {
        out.push(candidate(
            input,
            "empty_probe:vcs_refs".into(),
            Family::EmptyProbe,
            Params {
                probes: probes.iter().filter_map(|o| o.probe).collect(),
                ..Params::default()
            },
            probes.iter().map(|o| o.call).collect(),
        ));
    }
    let mut merged: BTreeMap<String, Candidate> = BTreeMap::new();
    for c in out.into_iter().filter(super::lint::validate) {
        if let Some(existing) = merged.get_mut(&c.id) {
            merge(existing, c);
        } else {
            merged.insert(c.id.clone(), c);
        }
    }
    merged.into_values().collect()
}
fn merge_counts(a: &mut BTreeMap<String, u32>, b: BTreeMap<String, u32>) {
    for (key, n) in b {
        let count = a.entry(key).or_default();
        *count = count.saturating_add(n);
    }
}
pub(super) fn merge(a: &mut Candidate, b: Candidate) {
    if a.scope == Scope::Neutral {
        a.scope = b.scope;
    }
    merge_counts(&mut a.params.good, b.params.good);
    merge_counts(&mut a.params.bad, b.params.bad);
    merge_counts(&mut a.params.hosts, b.params.hosts);
    for (e, n) in b.params.errors {
        let count = a.params.errors.entry(e).or_default();
        *count = count.saturating_add(n);
    }
    a.params.probes.extend(b.params.probes);
    a.params.examples.extend(b.params.examples);
    for evidence in b.evidence {
        if let Some(existing) = a
            .evidence
            .iter_mut()
            .find(|e| e.session_id == evidence.session_id)
        {
            existing.call_indices.extend(evidence.call_indices);
            existing.call_indices.sort_unstable();
            existing.call_indices.dedup();
        } else {
            a.evidence.push(evidence);
        }
    }
}
/// Selects a working form with a positive record, preferring forms that preserve cwd.
#[must_use]
pub fn working_form(candidate: &Candidate) -> Option<&str> {
    working_from(&candidate.params)
}
fn working_from(p: &Params) -> Option<&str> {
    let positive: Vec<_> = p
        .forms
        .iter()
        .filter_map(|(form, &n)| {
            let failed = p.form_failures.get(form).copied().unwrap_or(0);
            (n > failed).then_some((form, n.saturating_sub(failed), n))
        })
        .collect();
    let stable = positive.iter().any(|(form, _, _)| !moves_shell(form));
    positive
        .into_iter()
        .filter(|(form, _, _)| !stable || !moves_shell(form))
        .max_by_key(|(_, net, total)| (*net, *total))
        .map(|(form, _, _)| form.as_str())
}
pub(super) fn moves_shell(form: &str) -> bool {
    form.starts_with("cd ") && form.contains(" && ")
}
pub(super) fn best(counts: &BTreeMap<String, u32>) -> Option<&str> {
    counts
        .iter()
        .max_by_key(|(_, n)| *n)
        .map(|(s, _)| s.as_str())
}
/// A code verdict; decision evidence can add a contradiction but never erase one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Verdict {
    Confirm,
    Contradict,
    // SPEC 7.1: a named working form failed, distinct from TE's tentative bare-runner success.
    ContradictWorking,
}
#[derive(Debug, Clone, Copy, Default)]
pub(super) struct Exercise {
    pub verdict: Option<Verdict>,
    pub fired: bool,
    pub exercised: bool,
}
pub(super) fn exercise(
    fact: &mut Fact,
    input: &Observations,
    injected: bool,
    record_forms: bool,
) -> Exercise {
    let p = &mut fact.candidate.params;
    match fact.candidate.family {
        Family::FormRepair => exercise_form(p, input, injected, record_forms),
        Family::ArgumentRepair => exercise_argument(p, input, injected),
        Family::EnvValue => exercise_env(p, input, injected),
        Family::Boundary => exercise_boundary(p, input),
        Family::EmptyProbe => exercise_probe(p, input),
    }
}
fn exercise_form(
    p: &mut Params,
    input: &Observations,
    injected: bool,
    record_forms: bool,
) -> Exercise {
    let recommended = working_from(p).map(str::to_owned);
    let seq: Vec<_> = input
        .observations
        .iter()
        .filter(|o| Some(o.intent) == p.intent)
        .collect();
    let ok: BTreeSet<_> = seq
        .iter()
        .filter(|o| typed_ok(o))
        .map(|o| o.form.as_str())
        .collect();
    let failed: BTreeSet<_> = seq
        .iter()
        .filter(|o| observe::form_failed(o) && !o.assigned && !ok.contains(o.form.as_str()))
        .map(|o| o.form.as_str())
        .collect();
    let recommended_failed = recommended
        .as_deref()
        .is_some_and(|form| failed.contains(form));
    if record_forms && let Some(intent) = p.intent {
        for form in ok {
            if repairs(form, &p.bad_form, intent) && super::lint::command(form) {
                add(&mut p.forms, form);
            }
        }
    }
    for form in failed {
        if p.forms.contains_key(form) {
            add(&mut p.form_failures, form);
        }
    }
    let a_failed = seq
        .iter()
        .any(|o| o.form == p.bad_form && observe::form_failed(o));
    let b_ran = seq.iter().any(|o| {
        (p.forms.contains_key(&o.form) || !observe::is_bare(&o.form)) && observe::form_worked(o)
    });
    let a_runner_ok = input
        .observations
        .iter()
        .any(|o| o.intent == Intent::RunTests && o.form == p.bad_form && observe::form_worked(o));
    Exercise {
        exercised: !seq.is_empty(),
        fired: a_failed || (injected && b_ran),
        verdict: if recommended_failed {
            Some(Verdict::ContradictWorking)
        } else if a_failed || (b_ran && !a_runner_ok) {
            Some(Verdict::Confirm)
        } else if a_runner_ok {
            Some(Verdict::Contradict)
        } else {
            None
        },
    }
}
fn exercise_argument(p: &mut Params, input: &Observations, injected: bool) -> Exercise {
    let runs: Vec<_> = input
        .observations
        .iter()
        .filter(|o| o.intent == Intent::RunTests && o.form == p.form && !o.labels.is_empty())
        .collect();
    let bad = |lab: &str| match &p.rule {
        Some(LabelRule::Separator) => lab.contains("::"),
        Some(LabelRule::Strip(pre)) => lab.starts_with(pre),
        None => false,
    };
    let bad_failed = runs
        .iter()
        .any(|o| o.label_error && o.labels.iter().any(|s| bad(s)));
    let bad_loaded = runs
        .iter()
        .any(|o| o.loaded && o.labels.iter().any(|s| bad(s)));
    let good_loaded = runs
        .iter()
        .any(|o| o.loaded && o.labels.iter().any(|s| !bad(s)));
    Exercise {
        exercised: !runs.is_empty(),
        fired: bad_failed || (injected && good_loaded),
        verdict: if bad_loaded {
            Some(Verdict::Contradict)
        } else if bad_failed || good_loaded {
            Some(Verdict::Confirm)
        } else {
            None
        },
    }
}
fn exercise_env(p: &mut Params, input: &Observations, injected: bool) -> Exercise {
    let good = best(&p.good);
    let asked = input.env.iter().any(|c| c.needed.contains(&p.variable));
    let good_ok = input
        .env
        .iter()
        .any(|c| c.sets.get(&p.variable).map(String::as_str) == good && c.ok);
    let good_failed = input.env.iter().any(|c| {
        c.sets.get(&p.variable).map(String::as_str) == good && c.needed.contains(&p.variable)
    });
    Exercise {
        exercised: asked || input.env.iter().any(|c| c.sets.contains_key(&p.variable)),
        fired: asked || (injected && good_ok),
        verdict: if good_failed {
            Some(Verdict::Contradict)
        } else if good_ok {
            Some(Verdict::Confirm)
        } else {
            None
        },
    }
}
fn exercise_boundary(p: &mut Params, input: &Observations) -> Exercise {
    let fetches: Vec<_> = input
        .observations
        .iter()
        .filter(|o| {
            o.intent == Intent::FetchRemote && o.hosts.iter().any(|h| p.hosts.contains_key(h))
        })
        .collect();
    let success = fetches.iter().any(|o| o.outcome == Outcome::Ran);
    let failure = fetches
        .iter()
        .any(|o| matches!(o.outcome,Outcome::Error(e) if observe::network(e)));
    Exercise {
        exercised: !fetches.is_empty(),
        fired: failure,
        verdict: if success {
            Some(Verdict::Contradict)
        } else if failure {
            Some(Verdict::Confirm)
        } else {
            None
        },
    }
}
fn exercise_probe(_p: &mut Params, input: &Observations) -> Exercise {
    let probes: Vec<_> = input
        .observations
        .iter()
        .filter(|o| {
            o.intent == Intent::ProbeVcsRefs && matches!(o.outcome, Outcome::Hit | Outcome::Empty)
        })
        .collect();
    Exercise {
        exercised: !probes.is_empty(),
        fired: !probes.is_empty(),
        verdict: if probes.iter().any(|o| o.outcome == Outcome::Hit) {
            Some(Verdict::Contradict)
        } else if probes.is_empty() {
            None
        } else {
            Some(Verdict::Confirm)
        },
    }
}
