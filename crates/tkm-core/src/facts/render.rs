// Portions ported from microsoft/TokenEconomizer (tokeneconomizer/microskills/envfacts.py, commit d0675a3),
// Copyright (c) Microsoft Corporation, licensed under the Apache License, Version 2.0.
//! Code-owned templates and bounded fact blocks.

use super::{
    Candidate, FactsConfig, FactsState, Family, LabelRule, Scope, extract, lint,
    observe::ErrorClass,
};
use crate::model::Host;
use std::collections::BTreeMap;
/// Header is a code literal and counts toward the token cap.
pub const HEADER: &str = "# environment facts learned from earlier sessions in this repository";
fn usage(form: &str, args: &str) -> String {
    if extract::moves_shell(form) {
        format!("`({form}{args})`")
    } else {
        format!("`{form}{args}`")
    }
}
fn error(candidate: &Candidate) -> &'static str {
    match candidate
        .params
        .errors
        .iter()
        .max_by_key(|(_, n)| *n)
        .map(|(e, _)| e)
    {
        Some(ErrorClass::ModuleMissing) => "ModuleNotFoundError",
        Some(ErrorClass::CommandMissing) => "command not found",
        Some(ErrorClass::ScriptMissing) => "a file-not-found error",
        Some(ErrorClass::PermissionDenied) => "permission denied",
        Some(ErrorClass::UsageError) => "a usage error",
        _ => "environment error",
    }
}
fn join(items: &[String]) -> String {
    match items.split_last() {
        None => String::new(),
        Some((last, [])) => last.clone(),
        Some((last, rest)) => format!("{} and {last}", rest.join(", ")),
    }
}
/// Renders a validated family claim, or nothing when parameters are unsafe/incomplete.
#[must_use]
pub fn claim(candidate: &Candidate) -> String {
    if !lint::validate(candidate) {
        return String::new();
    }
    let p = &candidate.params;
    let text=match candidate.family {
        Family::FormRepair=>{
            let Some(form)=extract::working_form(candidate) else {return String::new();};
            format!("{} fails here with {}; {} works.",usage(&p.bad_form," …"),error(candidate),usage(form," …"))
        },
        Family::Boundary=>{
            let mut hosts:Vec<_>=p.hosts.iter().collect();hosts.sort_by(|a,b|b.1.cmp(a.1).then_with(||a.0.cmp(b.0)));
            let hosts:Vec<_>=hosts.into_iter().take(3).map(|(h,_)|h.clone()).collect();
            format!("Outbound connections to {} are refused here; nothing can be fetched from them.",join(&hosts))
        },
        Family::EmptyProbe=>"This checkout has no git remote and no branches, tags or unreachable objects beyond HEAD; its history ends at the checked-out commit.".into(),
        Family::ArgumentRepair=>{
            if p.rule==Some(LabelRule::Separator){format!("{} takes dotted test labels (`module.Class.test`, not `module::Class::test`).",usage(&p.form,""))}
            else if let Some(LabelRule::Strip(prefix))=&p.rule{
                let Some((bad,good))=p.examples.iter().min_by_key(|(bad,_)|bad.len()) else {return String::new();};
                format!("{} takes test labels without the `{prefix}` prefix (`{good}`, not `{bad}`).",usage(&p.form,""))
            }else{return String::new();}
        },
        Family::EnvValue=>{
            let Some(good)=extract::best(&p.good) else {return String::new();};
            let bad=p.bad.keys().find(|bad|bad.as_str()!=good);
            let ending=bad.map_or_else(||".".into(),|bad|format!(", `{bad}` does not."));
            format!("Code that imports the project needs `{}` set here: `{}={good}` works{ending}",p.variable,p.variable)
        },
    };
    if lint::lint_ok(&text) {
        text
    } else {
        String::new()
    }
}
fn root(form: &str) -> String {
    if let Some((prefix, _)) = form.split_once(" && ") {
        return prefix.into();
    }
    if ["conda run", "poetry run", "pipenv run", "uv run", "npx"]
        .iter()
        .any(|pre| form.starts_with(pre))
    {
        return form
            .split_whitespace()
            .take(3)
            .collect::<Vec<_>>()
            .join(" ");
    }
    let head = form.split_whitespace().next().unwrap_or("");
    head.rsplit_once('/').map_or(head, |(dir, _)| dir).into()
}
fn merged_lines(chosen: &[&Candidate]) -> Vec<String> {
    let mut groups: BTreeMap<String, Vec<&Candidate>> = BTreeMap::new();
    let mut lines = Vec::new();
    for candidate in chosen {
        if candidate.family == Family::FormRepair {
            groups
                .entry(root(extract::working_form(candidate).unwrap_or("")))
                .or_default()
                .push(candidate);
        } else {
            lines.push(claim(candidate));
        }
    }
    let mut merged = Vec::new();
    for fs in groups.into_values() {
        if fs.len() == 1 {
            merged.push(claim(fs[0]));
            continue;
        }
        let bad: Vec<_> = fs.iter().map(|f| usage(&f.params.bad_form, " …")).collect();
        let mut errors: Vec<_> = fs.iter().map(|f| error(f)).collect();
        errors.sort_unstable();
        errors.dedup();
        let mut good: Vec<_> = fs
            .iter()
            .filter_map(|f| extract::working_form(f))
            .map(|f| usage(f, " …"))
            .collect();
        good.sort();
        good.dedup();
        merged.push(format!(
            "{} fail here ({}); {} {}.",
            join(&bad),
            errors.join(", "),
            join(&good),
            if good.len() == 1 { "works" } else { "work" }
        ));
    }
    merged.extend(lines);
    merged
}
fn block(chosen: &[&Candidate]) -> String {
    format!(
        "{HEADER}\n{}\n",
        merged_lines(chosen)
            .into_iter()
            .map(|line| format!("- {line}"))
            .collect::<Vec<_>>()
            .join("\n")
    )
}
fn priority(candidate: &Candidate) -> u8 {
    match candidate.family {
        Family::FormRepair => 0,
        Family::ArgumentRepair => 1,
        Family::EnvValue => 2,
        Family::EmptyProbe => 3,
        Family::Boundary => 4,
    }
}
/// Renders eligible facts for a harness, merging equivalent repairs and enforcing caps.
///
/// The complete block (header, bullets and newlines) uses at most SPEC 4.3's
/// `max_tokens * 4` characters, TE's deterministic token estimate.
#[must_use]
pub fn render(state: &FactsState, host: Host, cfg: &FactsConfig) -> String {
    let mut eligible: Vec<_> = state
        .facts
        .values()
        .filter(|f| {
            f.life.injectable(cfg) && matches!(f.candidate.scope, Scope::Neutral)
                || f.life.injectable(cfg) && f.candidate.scope == Scope::Host(host)
        })
        .filter(|f| !claim(&f.candidate).is_empty())
        .collect();
    eligible.sort_by(|a, b| {
        priority(&a.candidate)
            .cmp(&priority(&b.candidate))
            .then_with(|| {
                let ai = a.candidate.params.intent == Some(super::observe::Intent::RunInterpreter);
                let bi = b.candidate.params.intent == Some(super::observe::Intent::RunInterpreter);
                bi.cmp(&ai)
            })
            .then_with(|| {
                let rate = |f: &super::Fact| {
                    f64::from(f.life.fire_num) / f64::from(f.life.fire_den.max(1))
                };
                rate(b).total_cmp(&rate(a))
            })
            .then_with(|| a.candidate.id.cmp(&b.candidate.id))
    });
    let mut chosen = Vec::new();
    for fact in eligible {
        if chosen.len() >= cfg.max_facts {
            break;
        }
        chosen.push(&fact.candidate);
        if block(&chosen).chars().count() > cfg.max_tokens.saturating_mul(4) {
            chosen.pop();
        }
    }
    if chosen.is_empty() {
        String::new()
    } else {
        block(&chosen)
    }
}
