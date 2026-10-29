// Portions ported from microsoft/TokenEconomizer (tokeneconomizer/microskills/observe.py, commit d0675a3),
// Copyright (c) Microsoft Corporation, licensed under the Apache License, Version 2.0.
//! Command classification and conservative output attribution.
use crate::model::{Host, Session};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Shell intent, using TE's GENERIC vocabulary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Intent {
    /// Run tests.
    RunTests,
    /// Run a language interpreter.
    RunInterpreter,
    /// Build, lint, compile, or check.
    RunBuild,
    /// Use a package tool.
    PackageTool,
    /// Fetch from a remote host.
    FetchRemote,
    /// Inspect references beyond the checkout.
    ProbeVcsRefs,
    /// Inspect the runtime environment.
    ProbeEnv,
}
/// Closed environmental error taxonomy; raw output is never a template parameter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorClass {
    /// A dependency module is missing.
    ModuleMissing,
    /// Executable unavailable.
    CommandMissing,
    /// Runner script unavailable at this location.
    ScriptMissing,
    /// Insufficient permissions.
    PermissionDenied,
    /// Unsupported invocation or option.
    UsageError,
    /// Host refused the connection.
    ConnectionRefused,
    /// Host name could not be resolved.
    NameResolution,
    /// Network route or timeout failure.
    Unreachable,
}
/// Typed result of one segment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    /// Insufficient segment-level evidence.
    #[default]
    Unknown,
    /// Executable ran, regardless of assertion failures.
    Ran,
    /// A discovery probe returned no reference.
    Empty,
    /// A discovery probe returned a reference.
    Hit,
    /// An environmental error occurred.
    Error(ErrorClass),
}
/// Narrow git discovery kinds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Probe {
    /// Configured remotes.
    Remote,
    /// Remote branches.
    Branch,
    /// Tags containing HEAD.
    Tag,
    /// Unreachable objects.
    Fsck,
    /// Stashes.
    Stash,
    /// Enumerated references.
    Refs,
}
/// One classified shell segment with output evidence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "Independent TE observation predicates, not lifecycle state flags"
)]
pub struct Observation {
    /// Source call index across turns.
    pub call: usize,
    /// Recognized operation class.
    pub intent: Intent,
    /// Command spelling, excluding labels and environment assignments.
    pub form: String,
    /// Executable spelling used for conservative attribution.
    pub head: String,
    /// Normalized simple segment.
    pub segment: String,
    /// URL hosts from the command only.
    pub hosts: Vec<String>,
    /// Discovery probe kind.
    pub probe: Option<Probe>,
    /// Restricted positional test labels.
    pub labels: Vec<String>,
    /// Runner could not load the requested label.
    pub label_error: bool,
    /// Positive evidence the label loaded and tests ran.
    pub loaded: bool,
    /// Positive evidence tests actually ran.
    pub tests_ran: bool,
    /// Assignment was necessary to how the command was typed.
    pub assigned: bool,
    /// Conservatively attributed result.
    pub outcome: Outcome,
    /// Reported process exit status, with digest fallback when absent.
    pub exit_code: Option<i32>,
}
/// Variable need/setting evidence for one shell call.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EnvCall {
    /// Source call index.
    pub call: usize,
    /// Variable names requested by output, never surrounding prose.
    pub needed: Vec<String>,
    /// Restricted literal values set in the command.
    pub sets: BTreeMap<String, String>,
    /// Call succeeded without a traceback or error.
    pub ok: bool,
}
/// Complete pure observation digest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Observations {
    /// Source session.
    pub session_id: String,
    /// Originating harness.
    pub host: Host,
    /// Incomplete sessions cannot train facts.
    pub partial: bool,
    /// Host tool names used to restrict tool-specific claims.
    pub tool_names: Vec<String>,
    /// Classified shell segments.
    pub observations: Vec<Observation>,
    /// Environment need/setting records.
    pub env: Vec<EnvCall>,
    /// Edited file basenames of every language.
    pub edited: Vec<String>,
}

/// TE GENERIC vocabulary, with names retained verbatim from observe.py.
#[derive(Debug, Clone, Copy)]
pub struct Vocabulary {
    /// Language runtimes.
    pub interpreters: &'static [&'static str],
    /// Canonical shebang-run interpreter.
    pub canonical_interpreter: &'static str,
    /// Bare test runners.
    pub runner_heads: &'static [&'static str],
    /// Recognized runner script suffixes.
    pub runner_scripts: &'static [&'static str],
    /// Interpreter modules that run tests.
    pub runner_modules: &'static [&'static str],
    /// Package managers and toolchain managers.
    pub package_tools: &'static [&'static str],
    /// URL fetch tools.
    pub fetch_heads: &'static [&'static str],
    /// Runtime discovery commands.
    pub env_probe_heads: &'static [&'static str],
    /// Tools supporting a test/build subcommand.
    pub task_heads: &'static [&'static str],
    /// Build and check executables.
    pub build_heads: &'static [&'static str],
}
/// Exact union of TE's GENERIC names, independent of repository language detection.
pub const GENERIC: Vocabulary = Vocabulary {
    interpreters: &[
        "python", "python2", "python3", "pypy", "pypy3", "node", "nodejs", "deno", "bun", "tsx",
        "ts-node", "ruby", "perl", "php", "Rscript", "julia", "lua", "elixir",
    ],
    canonical_interpreter: "python",
    runner_heads: &[
        "pytest",
        "py.test",
        "tox",
        "nox",
        "nosetests",
        "trial",
        "jest",
        "vitest",
        "mocha",
        "playwright",
        "cypress",
        "ava",
        "karma",
        "rspec",
        "phpunit",
        "ctest",
        "bats",
    ],
    runner_scripts: &["runtests.py", "bin/test", "bin/doctest", "manage.py"],
    runner_modules: &["pytest", "unittest", "nose", "django", "tox", "nox"],
    package_tools: &[
        "pip", "pip3", "conda", "mamba", "poetry", "pipenv", "uv", "apt", "apt-get", "npm", "yarn",
        "pnpm", "corepack", "gem", "bundle", "composer", "cargo", "go", "mvn", "gradle", "dotnet",
        "brew", "rustup", "pyenv", "nvm", "asdf",
    ],
    fetch_heads: &["curl", "wget"],
    env_probe_heads: &["which", "command", "type", "whereis"],
    task_heads: &[
        "make", "npm", "yarn", "pnpm", "cargo", "go", "mvn", "gradle", "gradlew", "dotnet", "mix",
        "swift", "rake",
    ],
    build_heads: &[
        "make",
        "cmake",
        "ninja",
        "bazel",
        "tsc",
        "eslint",
        "prettier",
        "ruff",
        "flake8",
        "pylint",
        "mypy",
        "pyright",
        "black",
        "isort",
        "golangci-lint",
        "gcc",
        "g++",
        "clang",
        "javac",
        "webpack",
        "vite",
    ],
};
const BUILD_SUBS: &[&str] = &[
    "build",
    "check",
    "compile",
    "lint",
    "vet",
    "typecheck",
    "type-check",
    "clippy",
    "package",
    "verify",
    "fmt",
    "format",
];
const PASSTHROUGH: &[&str] = &[
    "cd", "echo", "head", "tail", "cat", "sort", "uniq", "tee", "true", "printf", "less", "more",
];
const ERRORS: &[(ErrorClass, &str)] = &[
    (
        ErrorClass::ModuleMissing,
        r"(?i)ModuleNotFoundError|No module named|Cannot find module|ERR_MODULE_NOT_FOUND",
    ),
    (
        ErrorClass::CommandMissing,
        r"(?i)command not found|: not found\b|is not recognized",
    ),
    (
        ErrorClass::ScriptMissing,
        r"(?i)can't open file .*No such file or directory",
    ),
    (
        ErrorClass::PermissionDenied,
        r"(?i)Permission denied|EACCES",
    ),
    (
        ErrorClass::UsageError,
        r"(?im)^usage:|unrecognized arguments|no such option|error: argument|unknown option",
    ),
    (
        ErrorClass::ConnectionRefused,
        r"(?i)Connection refused|Failed to connect to|couldn't connect to|Could not connect|ECONNREFUSED",
    ),
    (
        ErrorClass::NameResolution,
        r"(?i)Could not resolve host|Temporary failure in name resolution|Name or service not known|getaddrinfo|nodename nor servname|ENOTFOUND",
    ),
    (
        ErrorClass::Unreachable,
        r"(?i)Network is unreachable|No route to host|Connection timed out|Operation timed out|timed out after|ETIMEDOUT",
    ),
];

pub(super) fn matches(pattern: &str, text: &str) -> bool {
    regex::Regex::new(pattern).is_ok_and(|rx| rx.is_match(text))
}
fn capture(pattern: &str, text: &str, group: usize) -> Option<String> {
    let rx = regex::Regex::new(pattern).ok()?;
    rx.captures(text)?.get(group).map(|m| m.as_str().to_owned())
}
fn captures(pattern: &str, text: &str, group: usize) -> Vec<String> {
    let Ok(rx) = regex::Regex::new(pattern) else {
        return Vec::new();
    };
    rx.captures_iter(text)
        .filter_map(|c| c.get(group).map(|m| m.as_str().to_owned()))
        .collect()
}
fn unwrap(command: &str) -> String {
    let Some(inner) = capture(r"(?s)^/bin/(?:ba)?sh\s+-l?c\s+(.*)$", command.trim(), 1) else {
        return command.to_owned();
    };
    let inner = inner.trim();
    for q in ['\'', '"'] {
        if let Some(body) = inner.strip_prefix(q).and_then(|s| s.strip_suffix(q)) {
            return if q == '"' {
                body.replace("\\\"", "\"")
                    .replace("\\$", "$")
                    .replace("\\\\", "\\")
            } else {
                body.to_owned()
            };
        }
    }
    inner.to_owned()
}
fn elide_heredocs(command: &str) -> String {
    let mut out = String::new();
    let mut lines = command.split_inclusive('\n');
    while let Some(line) = lines.next() {
        if let Some(tag) = capture(r#"<<-?\s*['"]?(\w+)['"]?"#, line, 1) {
            out.push_str(line.trim_end_matches('\n'));
            out.push_str(" <heredoc>\n");
            for body in lines.by_ref() {
                if body.trim() == tag {
                    break;
                }
            }
        } else {
            out.push_str(line);
        }
    }
    out
}
/// Splits operators outside quotes; heredoc bodies are elided.
#[must_use]
pub fn segments(command: &str) -> Vec<String> {
    let elided = elide_heredocs(command);
    let mut chars = elided.chars().peekable();
    let (mut out, mut buf, mut quote) = (Vec::new(), String::new(), None);
    let mut expansions: Vec<(char, Option<char>)> = Vec::new();
    while let Some(c) = chars.next() {
        // Single quotes preserve backslashes. Everywhere else an escaped operator
        // or quote belongs to the current segment rather than changing its syntax.
        if quote != Some('\'') && c == '\\' {
            buf.push(c);
            if let Some(next) = chars.next() {
                buf.push(next);
            }
            continue;
        }
        if quote == Some('\'') {
            buf.push(c);
            if c == '\'' {
                quote = None;
            }
            continue;
        }
        if c == '$' && chars.peek().is_some_and(|next| matches!(next, '(' | '{')) {
            buf.push(c);
            if let Some(opener) = chars.next() {
                buf.push(opener);
                expansions.push((if opener == '(' { ')' } else { '}' }, quote.take()));
            }
            continue;
        }
        if c == '`' && quote == Some('"') {
            buf.push(c);
            expansions.push(('`', quote.take()));
            continue;
        }
        if quote == Some('"') {
            buf.push(c);
            if c == '"' {
                quote = None;
            }
            continue;
        }
        if expansions.last().is_some_and(|(closer, _)| *closer == c) {
            buf.push(c);
            if let Some((_, outer_quote)) = expansions.pop() {
                quote = outer_quote;
            }
        } else if c == '`' {
            buf.push(c);
            expansions.push(('`', quote.take()));
        } else if c == '(' && !expansions.is_empty() {
            buf.push(c);
            expansions.push((')', None));
        } else if c == '\'' || c == '"' {
            quote = Some(c);
            buf.push(c);
        } else if expansions.is_empty()
            && (matches!(c, '\n' | ';' | '|') || (c == '&' && chars.peek() == Some(&'&')))
        {
            out.push(std::mem::take(&mut buf));
            if (c == '&' || c == '|') && chars.peek() == Some(&c) {
                chars.next();
            }
        } else {
            buf.push(c);
        }
    }
    out.push(buf);
    out.into_iter()
        .map(|s| s.trim().trim_matches(['(', ')']).trim().to_owned())
        .filter(|s| !s.is_empty())
        .collect()
}
struct ShellWord<'a> {
    raw: &'a str,
    literal: String,
    end: usize,
}
fn shell_words(command: &str) -> Vec<ShellWord<'_>> {
    let (mut out, mut start, mut literal, mut quote) = (Vec::new(), None, String::new(), None);
    let mut chars = command.char_indices().peekable();
    while let Some((index, c)) = chars.next() {
        if quote != Some('\'') && c == '\\' {
            start.get_or_insert(index);
            if let Some((_, next)) = chars.next() {
                if quote == Some('"') && !['$', '`', '"', '\\', '\n'].contains(&next) {
                    literal.push('\\');
                }
                if next != '\n' {
                    literal.push(next);
                }
            } else {
                literal.push(c);
            }
        } else if let Some(q) = quote {
            if c == q {
                quote = None;
            } else {
                literal.push(c);
            }
        } else if c == '\'' || c == '"' {
            start.get_or_insert(index);
            quote = Some(c);
        } else if c.is_whitespace() || matches!(c, ';' | '|' | '&' | '(' | ')') {
            if let Some(begin) = start.take() {
                out.push(ShellWord {
                    raw: &command[begin..index],
                    literal: std::mem::take(&mut literal),
                    end: index,
                });
            }
        } else {
            start.get_or_insert(index);
            literal.push(c);
        }
    }
    if quote.is_some() {
        return Vec::new();
    }
    if let Some(begin) = start {
        out.push(ShellWord {
            raw: &command[begin..],
            literal,
            end: command.len(),
        });
    }
    out
}
fn has_expansion(text: &str) -> bool {
    text.contains("$(") || text.contains('`') || text.contains("${")
}
fn assignment_word(word: &ShellWord<'_>) -> Option<(String, String)> {
    let (name, _) = word.raw.split_once('=')?;
    if !matches(r"\A[A-Za-z_][A-Za-z0-9_]*\z", name) {
        return None;
    }
    let value = word.literal.strip_prefix(&format!("{name}="))?;
    Some((name.into(), value.into()))
}
fn assignments(command: &str) -> (String, bool) {
    let mut end = 0;
    for word in shell_words(command) {
        if assignment_word(&word).is_none() {
            break;
        }
        end = word.end;
    }
    (command[end..].trim_start().to_owned(), end > 0)
}
fn normalized(command: &str) -> (String, bool) {
    let (mut body, mut assigned) = assignments(command);
    if let Ok(rx) = regex::Regex::new(r"^rtk\s+(?:proxy\s+)?")
        && let Some(prefix) = rx.find(&body)
        && prefix.end() < body.len()
    {
        let (next, has_assignments) = assignments(&body[prefix.end()..]);
        body = next;
        assigned |= has_assignments;
    }
    (body, assigned)
}
fn normalized_path(path: &str) -> String {
    let absolute = path.starts_with('/');
    let mut parts = Vec::new();
    for p in path.split('/') {
        match p {
            "" | "." => {}
            ".." if parts.last().is_some_and(|p| *p != "..") => {
                parts.pop();
            }
            ".." if absolute => {}
            p => parts.push(p),
        }
    }
    format!("{}{}", if absolute { "/" } else { "" }, parts.join("/"))
}
fn cd_target(cwd: &str, target: &str, root: Option<&str>, top: Option<&str>) -> Option<String> {
    if target.is_empty() || target.starts_with(['~', '$', '-']) {
        return None;
    }
    let rel = if target.starts_with('/') {
        let root = normalized_path(root?);
        let at = normalized_path(target);
        if at == root {
            String::new()
        } else if let Some(rel) = at.strip_prefix(&format!("{root}/")) {
            rel.to_owned()
        } else if top.is_some_and(|top| normalized_path(top) == at) {
            return Some(String::new());
        } else {
            return None;
        }
    } else {
        normalized_path(format!("{cwd}/{target}").trim_start_matches('/'))
    };
    if rel == ".." || rel.starts_with("../") {
        if let (Some(root), Some(top)) = (root, top)
            && normalized_path(&format!("{root}/{rel}")) == normalized_path(top)
        {
            return Some(String::new());
        }
        return None;
    }
    Some(rel)
}
/// Classifies commands after rtk and rtk proxy normalization.
#[must_use]
pub fn classify_command(command: &str) -> Vec<Observation> {
    classify_command_at(command, 0, "", None, None)
}
/// Classifies using caller-supplied directories, without filesystem discovery.
///
/// Frozen `model::Call` has no per-call cwd. Log adapters can call this helper with
/// that metadata; [`observe`] uses only the session's supplied cwd and explicit cds.
#[must_use]
pub fn classify_command_at(
    command: &str,
    call: usize,
    cwd: &str,
    root: Option<&str>,
    top: Option<&str>,
) -> Vec<Observation> {
    let (mut out, mut carried, mut here) = (Vec::new(), String::new(), Some(cwd.to_owned()));
    let mut cd = if cwd.is_empty() {
        String::new()
    } else {
        format!("cd {cwd} &&")
    };
    for segment in segments(&unwrap(command)) {
        // Expansions are opaque evidence: their commands/values are never evaluated.
        if has_expansion(&segment) {
            continue;
        }
        let (seg, mut assigned) = normalized(&segment);
        if let Some(target) = capture(r#"^cd\s+['"]?([^\s;&|'\"]+)['"]?\s*$"#, &seg, 1) {
            here = here.as_ref().and_then(|h| cd_target(h, &target, root, top));
            cd = here.as_ref().map_or_else(
                || format!("cd {target} &&"),
                |h| {
                    if h.is_empty() {
                        String::new()
                    } else {
                        format!("cd {h} &&")
                    }
                },
            );
            continue;
        }
        if let Some(env) = capture(r"^(?:source|\.)\s+\S*activate\s+([\w.-]+)\s*$", &seg, 1) {
            carried = format!("source activate {env} &&");
            continue;
        }
        if let Some(env) = capture(r"^(?:conda|mamba)\s+activate\s+([\w.-]+)\s*$", &seg, 1) {
            carried = format!("conda activate {env} &&");
            continue;
        }
        let mut prefix = [cd.as_str(), carried.as_str()]
            .into_iter()
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join(" ");
        let body = unwrap_tool(&seg, &mut prefix);
        let words = shell_words(&body);
        let mut tokens: Vec<&str> = words.iter().map(|word| word.literal.as_str()).collect();
        while let Some(first) = tokens.first().copied() {
            if matches(r"^[A-Za-z_][A-Za-z0-9_]*=", first) {
                assigned = true;
                tokens.remove(0);
            } else if ["sudo", "env", "time", "exec", "nice", "nohup", "timeout"].contains(&first) {
                let n = if first == "timeout" {
                    2.min(tokens.len())
                } else {
                    1
                };
                tokens.drain(..n);
            } else {
                break;
            }
        }
        let Some(head) = tokens.first().copied() else {
            continue;
        };
        let base = head.rsplit('/').next().unwrap_or(head);
        let rest = &tokens[1..];
        if let Some((intent, expr, probe)) = classify(head, base, rest) {
            let form = format!("{prefix} {expr}").trim().to_owned();
            let hosts = if intent == Intent::FetchRemote {
                captures(r#"https?://([^/\s'"`]+)"#, &body, 1)
            } else {
                Vec::new()
            };
            let labels = if intent == Intent::RunTests {
                test_labels(&seg, &form)
            } else {
                Vec::new()
            };
            out.push(Observation {
                call,
                intent,
                form,
                head: head.to_owned(),
                segment: seg,
                hosts,
                probe,
                labels,
                label_error: false,
                loaded: false,
                tests_ran: false,
                assigned,
                outcome: Outcome::Unknown,
                exit_code: None,
            });
        }
    }
    out
}
fn unwrap_tool(segment: &str, prefix: &mut String) -> String {
    let mut body = segment.to_owned();
    if let Ok(rx) = regex::Regex::new(
        r"^(?:conda|mamba)\s+run\s+(?:--no-capture-output\s+|--live-stream\s+)*(?:-n|--name)\s+([\w.-]+)\s+",
    ) && let Some(c) = rx.captures(&body)
        && let (Some(full), Some(env)) = (c.get(0), c.get(1))
    {
        format!("{prefix} conda run -n {}", env.as_str())
            .trim()
            .clone_into(prefix);
        body = body[full.end()..].to_owned();
    }
    if let Ok(rx) = regex::Regex::new(
        r"^(poetry|pipenv|uv|npx|bundle|yarn|pnpm)\s+(?:run|exec)\s+|^(npx)\s+(?:(?:--yes|-y|--no-install|--no)\s+)*",
    ) && let Some(c) = rx.captures(&body)
        && let (Some(full), Some(tool)) = (c.get(0), c.get(1).or_else(|| c.get(2)))
    {
        let wrapper = if c.get(1).is_some() {
            format!("{} run", tool.as_str())
        } else {
            "npx".to_owned()
        };
        format!("{prefix} {wrapper}").trim().clone_into(prefix);
        body = body[full.end()..].to_owned();
    }
    body
}
fn classify(head: &str, base: &str, rest: &[&str]) -> Option<(Intent, String, Option<Probe>)> {
    let result = |intent, form: String| Some((intent, form, None));
    if GENERIC.interpreters.contains(&base) || matches(r"^(?:python|node)\d(?:\.\d+)?$", base) {
        if !rest.is_empty()
            && rest
                .iter()
                .all(|s| ["--version", "-V", "-VV", "-v"].contains(s))
        {
            return result(Intent::ProbeEnv, head.into());
        }
        if let Some(i) = rest.iter().position(|s| *s == "-m")
            && let Some(module) = rest
                .get(i + 1)
                .filter(|s| GENERIC.runner_modules.contains(s))
        {
            return result(Intent::RunTests, format!("{head} -m {module}"));
        }
        if let Some(script) = rest.iter().find(|s| !s.starts_with('-')).filter(|s| {
            GENERIC
                .runner_scripts
                .iter()
                .any(|suffix| s.ends_with(suffix))
        }) && (script.rsplit('/').next() != Some("manage.py") || rest.contains(&"test"))
        {
            return result(Intent::RunTests, runner_script_form(head, script));
        }
        return result(Intent::RunInterpreter, head.into());
    }
    if GENERIC.runner_heads.contains(&base) {
        return result(Intent::RunTests, head.into());
    }
    if GENERIC
        .runner_scripts
        .iter()
        .any(|s| head == *s || head.ends_with(&format!("/{s}")))
        && (base != "manage.py" || rest.contains(&"test"))
    {
        return result(
            Intent::RunTests,
            runner_script_form(GENERIC.canonical_interpreter, head),
        );
    }
    if GENERIC.task_heads.contains(&base) && rest.first() == Some(&"test") {
        return result(Intent::RunTests, format!("{base} test"));
    }
    if ["npm", "yarn", "pnpm", "bun"].contains(&base)
        && rest.first() == Some(&"run")
        && let Some(script) = rest.get(1)
    {
        let kind = script.split(':').next().unwrap_or(script);
        if kind == "test" {
            return result(Intent::RunTests, format!("{base} run {script}"));
        }
        if BUILD_SUBS.contains(&kind) {
            return result(Intent::RunBuild, format!("{base} run {script}"));
        }
    }
    if GENERIC.task_heads.contains(&base) && rest.first().is_some_and(|s| BUILD_SUBS.contains(s)) {
        return result(Intent::RunBuild, format!("{base} {}", rest[0]));
    }
    if GENERIC.build_heads.contains(&base) {
        return result(Intent::RunBuild, base.into());
    }
    if GENERIC.env_probe_heads.contains(&base) {
        return result(Intent::ProbeEnv, base.into());
    }
    if GENERIC.fetch_heads.contains(&base) {
        return result(Intent::FetchRemote, base.into());
    }
    if base == "git" {
        return classify_git(rest);
    }
    if GENERIC.package_tools.contains(&base) {
        if ["pip", "npm", "yarn", "pnpm"]
            .iter()
            .any(|p| base.starts_with(p))
            && rest
                .first()
                .is_some_and(|s| ["install", "download", "add", "ci"].contains(s))
        {
            return result(Intent::FetchRemote, format!("{base} {}", rest[0]));
        }
        return result(Intent::PackageTool, base.into());
    }
    None
}
fn runner_script_form(head: &str, script: &str) -> String {
    format!(
        "{head} {}{}",
        script.trim_start_matches("./"),
        if script.rsplit('/').next() == Some("manage.py") {
            " test"
        } else {
            ""
        }
    )
}
fn classify_git(rest: &[&str]) -> Option<(Intent, String, Option<Probe>)> {
    let result = |intent, form: String| Some((intent, form, None));

    let sub = *rest.first()?;
    if ["fetch", "clone", "pull", "ls-remote"].contains(&sub) {
        return result(Intent::FetchRemote, format!("git {sub}"));
    }
    let args = &rest[1..];
    let probe = match sub {
        "remote" => Some(Probe::Remote),
        "branch"
            if args
                .iter()
                .any(|a| ["-a", "-r", "--all", "--remotes", "-ar", "-ra"].contains(a)) =>
        {
            Some(Probe::Branch)
        }
        "tag" if args.contains(&"--contains") => Some(Probe::Tag),
        "fsck" => Some(Probe::Fsck),
        "stash" if args.first() == Some(&"list") => Some(Probe::Stash),
        "show-ref" | "for-each-ref" => Some(Probe::Refs),
        _ => None,
    };
    probe.map(|p| (Intent::ProbeVcsRefs, format!("git {sub}"), Some(p)))
}
fn test_labels(segment: &str, form: &str) -> Vec<String> {
    let words = shell_words(segment);
    let tokens: Vec<&str> = words.iter().map(|word| word.literal.as_str()).collect();
    let last = form.split_whitespace().last().unwrap_or("");
    let start = tokens
        .iter()
        .position(|t| {
            *t == last || t.trim_start_matches("./") == last || t.ends_with(&format!("/{last}"))
        })
        .map_or(tokens.len(), |i| i + 1);
    let (mut out, mut skip, mut prev) = (Vec::new(), false, "");
    for t in &tokens[start..] {
        if skip {
            skip = false;
            prev = t;
            continue;
        }
        if t.starts_with(['>', '<', '|']) || t.starts_with("2>") {
            skip = [">", ">>", "2>", "<"].contains(t);
            prev = t;
            continue;
        }
        let structured = t.contains('.') || t.contains("::") || t.contains('/');
        if !t.starts_with('-')
            && matches(r"^[A-Za-z_][\w.\-/]*(?:::[\w.\-\[\]]+)*$", t)
            && (structured || !prev.starts_with('-') || prev.contains('='))
        {
            out.push((*t).to_owned());
        }
        prev = t;
    }
    out
}
/// Reads environmental errors in TE precedence order.
#[must_use]
pub fn error_class(output: &str) -> Option<ErrorClass> {
    ERRORS
        .iter()
        .find(|(_, rx)| matches(rx, output))
        .map(|(e, _)| *e)
}
pub(super) fn network(error: ErrorClass) -> bool {
    matches!(
        error,
        ErrorClass::ConnectionRefused | ErrorClass::NameResolution | ErrorClass::Unreachable
    )
}
/// Relates labels by separator replacement or prefix removal only.
#[must_use]
pub fn label_rule(bad: &str, good: &str) -> Option<super::LabelRule> {
    if bad.contains("::") && !good.contains("::") {
        let dotted = bad.replace("::", ".");
        if good == dotted
            || dotted.starts_with(&format!("{good}."))
            || good.starts_with(&format!("{dotted}."))
        {
            return Some(super::LabelRule::Separator);
        }
    }
    for sep in ['.', '/'] {
        if let Some((first, rest)) = bad.split_once(sep) {
            let prefix = format!("{first}{sep}");
            if !rest.is_empty()
                && !good.starts_with(&prefix)
                && (good == rest
                    || rest.starts_with(&format!("{good}{sep}"))
                    || good.starts_with(&format!("{rest}{sep}")))
            {
                return Some(super::LabelRule::Strip(prefix));
            }
        }
    }
    None
}
/// Returns whether a label failed to load, excluding missing runner modules.
#[must_use]
pub fn label_failed(labels: &[String], output: &str) -> bool {
    if matches(
        r"(?i)Failed to import test module|unittest\.loader\._FailedTest|ERROR: (?:file or directory )?not found|no tests ran|module '[\w.]+' has no attribute",
        output,
    ) {
        return true;
    }
    let modules = captures(r#"No module named ['"]?([\w.]+)"#, output, 1);
    modules.iter().any(|m| {
        labels
            .iter()
            .any(|label| label.split(['.', '/', ':']).next() == m.split('.').next())
    })
}
fn loaded(output: &str) -> bool {
    matches(
        r"\bRan [1-9]\d* tests?\b|\b[1-9]\d* (?:passed|failed|passing|failing)\b|\bcollected [1-9]\d* items?\b",
        output,
    )
}
fn tests_ran(output: &str) -> bool {
    loaded(output)
        || matches(
            r"(?m)^ok\s+\S+\s+[\d.]+s\b|^--- (?:PASS|FAIL)\b|\btest result: (?:ok|FAILED)\b|\b[1-9]\d* examples?, \d+ failures?\b|\b(?:Passed|Failed)!\s+-|\bTests run: [1-9]\d*",
            output,
        )
}
fn probe_hit(probe: Probe, output: &str) -> bool {
    matches(
        match probe {
            Probe::Remote => r"(?m)^\S+\s+\S+\s+\((?:fetch|push)\)\s*$",
            Probe::Branch => r"(?m)^\s*remotes/\S+",
            Probe::Tag => r"(?m)^v?\d+(?:\.\d+)+(?:[a-z]+\d*|\.post\d+|-\S+)?\s*$",
            Probe::Fsck => {
                r"(?m)^(?:unreachable|dangling)\s+(?:commit|tree|blob|tag)\s+[0-9a-f]{7,}"
            }
            Probe::Stash => r"(?m)^stash@\{\d+\}",
            Probe::Refs => r"(?m)^[0-9a-f]{7,}\s+refs/",
        },
        output,
    )
}
fn shell_sets(command: &str) -> BTreeMap<String, String> {
    let mut sets = BTreeMap::new();
    for segment in segments(&unwrap(command)) {
        if has_expansion(&segment) {
            continue;
        }
        for word in shell_words(&segment) {
            if let Some((name, value)) = assignment_word(&word) {
                if matches(r"\A[A-Z_][A-Z0-9_]*\z", &name) && !has_expansion(&value) {
                    sets.insert(name, value);
                }
            } else if ![
                "env", "export", "sudo", "time", "exec", "nice", "nohup", "rtk", "proxy",
            ]
            .contains(&word.literal.as_str())
            {
                break;
            }
        }
    }
    sets
}
fn env_call(command: &str, output: &str, ok: bool, call: usize) -> EnvCall {
    let needed = captures(
        r#"environment variable\s+['"`]?([A-Z_][A-Z0-9_]*)"#,
        output,
        1,
    );
    let mut sets = shell_sets(command);
    let in_code = !has_expansion(command)
        && classify_command(command)
            .iter()
            .any(|o| o.intent == Intent::RunInterpreter);
    if let Ok(rx) = regex::Regex::new(
        r#"os\.environ(?:\.setdefault\(\s*|\[\s*)['"]([A-Z_][A-Z0-9_]*)['"]\s*(?:,|\]\s*=)\s*['"]([^'"]*)['"]"#,
    ) && in_code
    {
        for c in rx.captures_iter(command) {
            if let (Some(var), Some(value)) = (c.get(1), c.get(2)) {
                sets.insert(var.as_str().into(), value.as_str().into());
            }
        }
    }
    EnvCall {
        call,
        needed,
        sets,
        ok: ok
            && !matches(
                r"(?m)Traceback \(most recent call last\)|^\w*Error\b|\b[Ee]xit [Cc]ode[:=\s]+[1-9]",
                output,
            ),
    }
}
fn homogeneous(obs: &[Observation], command: &str) -> bool {
    let n = segments(&unwrap(command))
        .iter()
        .filter(|s| {
            !PASSTHROUGH.contains(
                &s.split_whitespace()
                    .next()
                    .unwrap_or("")
                    .rsplit('/')
                    .next()
                    .unwrap_or(""),
            )
        })
        .count();
    n == obs.len()
        && obs
            .first()
            .is_some_and(|first| obs.iter().all(|o| o.intent == first.intent))
}
fn fetch_homogeneous(command: &str, homogeneous: bool) -> bool {
    // Output producers can fabricate a successful fetch's entire digest. TE's
    // pass-through rule is retained for probes but tightened for reachability.
    homogeneous
        && !segments(&unwrap(command)).iter().any(|s| {
            ["echo", "printf", "cat"].contains(
                &s.split_whitespace()
                    .next()
                    .unwrap_or("")
                    .rsplit('/')
                    .next()
                    .unwrap_or(""),
            )
        })
}
fn attribute(obs: &mut [Observation], command: &str, output: &str, success: bool) {
    let homogeneous = homogeneous(obs, command);
    let fetch_homogeneous = fetch_homogeneous(command, homogeneous);
    let error = error_class(output);
    if let Some(error) = error {
        let targets: Vec<usize> = obs
            .iter()
            .enumerate()
            .filter(|(_, o)| !matches!(o.intent, Intent::ProbeEnv | Intent::ProbeVcsRefs))
            .map(|(i, _)| i)
            .collect();
        let error_line = ERRORS
            .iter()
            .find(|(e, _)| *e == error)
            .and_then(|(_, rx)| output.lines().find(|line| matches(rx, line)))
            .unwrap_or("");
        let mut chosen: Vec<usize> = targets
            .iter()
            .copied()
            .filter(|&i| {
                let o = &obs[i];
                if network(error) {
                    o.intent == Intent::FetchRemote
                        && (o.hosts.is_empty() || o.hosts.iter().any(|h| output.contains(h)))
                } else {
                    output.contains(&o.head)
                        || error_line.contains(o.head.rsplit('/').next().unwrap_or(""))
                }
            })
            .collect();
        if chosen.is_empty() && network(error) {
            chosen = targets
                .iter()
                .copied()
                .filter(|&i| obs[i].intent == Intent::FetchRemote)
                .collect();
        }
        if chosen.is_empty() && !success {
            chosen.extend(targets.last().copied());
        }
        for i in chosen {
            obs[i].outcome = Outcome::Error(error);
        }
        if success {
            for o in obs.iter_mut() {
                if o.outcome == Outcome::Unknown {
                    o.outcome = Outcome::Ran;
                }
            }
        }
    } else if success {
        for o in obs.iter_mut() {
            o.outcome = Outcome::Ran;
        }
    } else {
        let mut first = true;
        for o in obs.iter_mut() {
            if !matches!(o.intent, Intent::ProbeEnv | Intent::ProbeVcsRefs) {
                if first {
                    o.outcome = Outcome::Ran;
                }
                first = false;
            } else if o.intent == Intent::ProbeEnv {
                o.outcome = Outcome::Ran;
            }
        }
    }
    for o in obs {
        if let Some(probe) = o.probe {
            o.outcome = if !homogeneous {
                Outcome::Unknown
            } else if probe_hit(probe, output) {
                Outcome::Hit
            } else if success {
                Outcome::Empty
            } else {
                Outcome::Unknown
            };
        }
        if o.intent == Intent::FetchRemote
            && o.outcome == Outcome::Ran
            && (!fetch_homogeneous
                || !success
                || output.trim().is_empty()
                || error.is_some_and(network))
        {
            o.outcome = Outcome::Unknown;
        }
    }
}
/// Produces conservative observations from a normalized session.
///
/// `Call::args_digest` is normalized shell command text, not JSON. Explicit
/// `exit_code` is retained; TE-style output exit hints override success evidence.
#[must_use]
pub fn observe(session: &Session) -> Observations {
    let mut digest = Observations {
        session_id: session.session_id.clone(),
        host: session.host,
        partial: session.partial,
        tool_names: session
            .tool_catalog
            .iter()
            .map(|t| t.name.clone())
            .collect(),
        observations: Vec::new(),
        env: Vec::new(),
        edited: Vec::new(),
    };
    if session.partial {
        return digest;
    }
    let mut edited = std::collections::BTreeSet::new();
    for (idx, call) in session.turns.iter().flat_map(|t| &t.calls).enumerate() {
        for edit in &call.edits {
            if let Some(name) = edit.path.file_name().and_then(|p| p.to_str()) {
                edited.insert(name.to_owned());
            }
        }
        if ![
            "shell",
            "bash",
            "stop_bash",
            "exec_command",
            "functions.exec_command",
            "mcp__functions__exec_command",
        ]
        .contains(&call.tool.to_ascii_lowercase().as_str())
        {
            continue;
        }
        let out = &call.output_digest;
        if matches(
            r"\bcd: [^\n]*: (?:No such file or directory|Not a directory)",
            out,
        ) {
            continue;
        }
        let hint =
            capture(r"(?i)\bexit code[:=\s]+(\d+)", out, 1).and_then(|s| s.parse::<i32>().ok());
        let success = hint
            .or(call.exit_code)
            .map_or(!call.is_error, |code| code == 0);
        let env = env_call(&call.args_digest, out, success, idx);
        if !env.needed.is_empty() || !env.sets.is_empty() {
            digest.env.push(env);
        }
        let mut obs = classify_command_at(
            &call.args_digest,
            idx,
            "",
            session.cwd.to_str().filter(|s| !s.is_empty()),
            None,
        );
        for o in &mut obs {
            o.exit_code = call.exit_code.or(hint);
            if o.intent == Intent::RunTests {
                o.label_error = !o.labels.is_empty() && label_failed(&o.labels, out);
                o.loaded = !o.labels.is_empty() && !o.label_error && loaded(out);
                o.tests_ran = tests_ran(out);
            }
        }
        attribute(&mut obs, &call.args_digest, out, success);
        digest.observations.extend(obs);
    }
    digest.edited = edited.into_iter().collect();
    digest
}
pub(super) fn form_failed(o: &Observation) -> bool {
    matches!(o.outcome,Outcome::Error(e) if !network(e))
        && !o.label_error
        && !(o.intent == Intent::RunTests && o.outcome == Outcome::Error(ErrorClass::UsageError))
}
pub(super) fn form_worked(o: &Observation) -> bool {
    o.outcome == Outcome::Ran && (o.intent != Intent::RunTests || o.tests_ran)
}
pub(super) fn is_bare(form: &str) -> bool {
    let head = form.split_whitespace().next().unwrap_or("");
    !head.contains('/')
        && !form.contains(" activate ")
        && !form.contains(" run ")
        && !form.starts_with("npx ")
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use crate::facts::{self, FactsConfig, FactsState, SessionEvidence, lifecycle::Status};
    use crate::model::{Call, Turn};
    use std::collections::BTreeSet;

    fn session(command: &str, output: &str, code: i32) -> Session {
        let mut session = Session::new(Host::Claude);
        session.session_id = "review-regression".into();
        session.turns.push(Turn {
            calls: vec![Call {
                tool: "Bash".into(),
                args_digest: command.into(),
                output_digest: output.into(),
                exit_code: Some(code),
                ..Call::default()
            }],
            ..Turn::default()
        });
        session
    }

    #[test]
    fn native_git_reference_formats_are_hits() {
        for (command, output, probe) in [
            ("git remote", "origin\n", Probe::Remote),
            ("git remote", "origin\nupstream\n", Probe::Remote),
            (
                "git remote -v",
                "origin\thttps://example.org/repo.git (fetch)\norigin\thttps://example.org/repo.git (push)\n",
                Probe::Remote,
            ),
            ("git branch -r", "  origin/main\n", Probe::Branch),
            (
                "git branch --remotes",
                "  origin/HEAD -> origin/main\n  origin/main\n  upstream/topic/fix\n",
                Probe::Branch,
            ),
            (
                "git branch -a",
                "* main\n  remotes/origin/main\n",
                Probe::Branch,
            ),
            ("git tag --contains HEAD", "nightly\n", Probe::Tag),
            (
                "git tag --contains HEAD",
                "release/preview\nbuild-2026_10\nv1.2.3\n",
                Probe::Tag,
            ),
        ] {
            let digest = observe(&session(command, output, 0));
            assert_eq!(digest.observations.len(), 1, "{command}");
            assert_eq!(digest.observations[0].probe, Some(probe), "{command}");
            assert_eq!(
                digest.observations[0].outcome,
                Outcome::Hit,
                "{command}: {output:?}"
            );
        }
    }

    #[test]
    fn unrecognized_nonempty_vcs_output_abstains() {
        for command in [
            "git remote",
            "git branch -r",
            "git tag --contains HEAD",
            "git fsck",
            "git stash list",
            "git show-ref",
            "git for-each-ref",
        ] {
            let digest = observe(&session(command, "warning: probe output unavailable\n", 0));
            assert_eq!(digest.observations.len(), 1, "{command}");
            assert_eq!(
                digest.observations[0].outcome,
                Outcome::Unknown,
                "{command}"
            );
            assert!(facts::extract::extract(&digest).is_empty(), "{command}");
        }
    }

    #[test]
    fn invalid_git_reference_names_abstain() {
        for (command, output) in [
            ("git remote", "warning:\n"),
            ("git remote", "bad..name\n"),
            ("git branch -r", "  origin/main -> warning:\n"),
            ("git branch -r", "  origin/bad..name\n"),
            ("git tag --contains HEAD", "invalid tag name\n"),
            ("git tag --contains HEAD", "bad..name\n"),
        ] {
            let digest = observe(&session(command, output, 0));
            assert_eq!(digest.observations[0].outcome, Outcome::Unknown);
        }
    }

    #[test]
    fn only_empty_successful_vcs_output_counts_as_empty() {
        for command in [
            "git remote",
            "git branch -r",
            "git tag --contains HEAD",
            "git fsck",
        ] {
            for output in ["", " \t\n"] {
                for (code, expected) in [(0, Outcome::Empty), (1, Outcome::Unknown)] {
                    let digest = observe(&session(command, output, code));
                    assert_eq!(
                        digest.observations[0].outcome, expected,
                        "{command}, {code}"
                    );
                }
            }
        }
    }

    #[test]
    fn native_git_hits_never_confirm_empty_history() {
        for (command, output) in [
            ("git remote", "origin\n"),
            ("git branch -r", "  origin/main\n"),
            ("git tag --contains HEAD", "nightly\n"),
        ] {
            let mut state = FactsState::default();
            for index in 0..4 {
                let mut session = session(command, output, 0);
                session.session_id = format!("hit-{index}");
                state = facts::update(
                    state,
                    &SessionEvidence {
                        session,
                        injected_ids: BTreeSet::default(),
                        decisions: None,
                    },
                    &FactsConfig::default(),
                )
                .0;
            }
            let fact = &state.facts["empty_probe:vcs_refs"];
            assert_eq!(fact.life.confirmations, 0, "{command}");
            assert_eq!(fact.life.status, Status::Retired, "{command}");
        }
    }

    #[test]
    fn shell_comments_are_removed_before_segmentation() {
        for (command, expected) in [
            ("echo 4 passed # ;python -m pytest", vec!["echo 4 passed"]),
            ("# python -m pytest; git remote", vec![]),
            ("echo ok;# python -m pytest", vec!["echo ok"]),
            ("echo ok |# python -m pytest", vec!["echo ok"]),
            ("echo ok &&# python -m pytest", vec!["echo ok"]),
            (
                "echo ok # 'unterminated ; pytest\npython -m pytest # ;git remote",
                vec!["echo ok", "python -m pytest"],
            ),
            (
                "# ignored\necho ok # ignored\n# ignored\npython -m pytest",
                vec!["echo ok", "python -m pytest"],
            ),
            (
                "echo ok # ignored\\\npython -m pytest",
                vec!["echo ok", "python -m pytest"],
            ),
        ] {
            assert_eq!(segments(command), expected, "{command:?}");
        }
    }

    #[test]
    fn literal_hashes_do_not_start_shell_comments() {
        for first in [
            "echo a#b",
            "echo '4 # passed'",
            "echo \"4 # passed\"",
            r"echo \#literal",
            r"echo escaped\ #literal",
            "echo ''#literal",
            "echo a\\\n#literal",
            "echo 'multi\n# literal'",
        ] {
            let command = format!("{first}; python -m pytest");
            assert_eq!(segments(&command), vec![first, "python -m pytest"]);
            assert_eq!(classify_command(&command).len(), 1, "{command}");
        }
    }

    #[test]
    fn commented_commands_produce_no_observations_or_evidence() {
        for command in [
            "echo 4 passed # ;python -m pytest",
            "echo 4 passed # &&python -m pytest",
            "echo 4 passed # |python -m pytest",
            "# python -m pytest; git remote; export PYTHONPATH=src",
            "echo ok # ;export PYTHONPATH=src\n# ;python -c 'print(1)'",
        ] {
            assert!(classify_command(command).is_empty(), "{command}");
            let digest = observe(&session(command, "4 passed", 0));
            assert!(digest.observations.is_empty(), "{command}");
            assert!(digest.env.is_empty(), "{command}");
            assert!(facts::extract::extract(&digest).is_empty(), "{command}");
        }
    }

    #[test]
    fn commented_repairs_cannot_create_facts() {
        let mut input = session("pytest", "command not found", 127);
        input.turns[0].calls.extend(
            session("echo 4 passed # ;python -m pytest", "4 passed", 0)
                .turns
                .remove(0)
                .calls,
        );
        let digest = observe(&input);
        assert_eq!(digest.observations.len(), 1);
        assert_eq!(facts::extract::extract(&digest), []);
    }
}
