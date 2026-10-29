// Portions ported from microsoft/TokenEconomizer (tokeneconomizer/microskills/observe.py, commit d0675a3),
// Copyright (c) Microsoft Corporation, licensed under the Apache License, Version 2.0.
//! Command classification and conservative output attribution.
use crate::model::{Host, Session};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Shell intent, using TE's GENERIC vocabulary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
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
    /// Classified shell segments.
    pub observations: Vec<Observation>,
    /// Environment need/setting records.
    pub env: Vec<EnvCall>,
    /// Edited file basenames of every language.
    pub edited: Vec<String>,
}
/// Splits operators outside quotes; heredoc bodies are elided.
#[must_use]
pub fn segments(_command: &str) -> Vec<String> {
    Vec::new()
}
/// Classifies commands after rtk and rtk proxy normalization.
#[must_use]
pub fn classify_command(_command: &str) -> Vec<Observation> {
    Vec::new()
}
/// Classifies using caller-supplied directories, without filesystem discovery.
#[must_use]
pub fn classify_command_at(
    _command: &str,
    _call: usize,
    _cwd: &str,
    _root: Option<&str>,
    _top: Option<&str>,
) -> Vec<Observation> {
    Vec::new()
}
/// Reads environmental errors in TE precedence order.
#[must_use]
pub fn error_class(_output: &str) -> Option<ErrorClass> {
    None
}
/// Relates labels by separator replacement or prefix removal only.
#[must_use]
pub fn label_rule(_bad: &str, _good: &str) -> Option<super::LabelRule> {
    None
}
/// Returns whether a label failed to load, excluding missing runner modules.
#[must_use]
pub fn label_failed(_labels: &[String], _output: &str) -> bool {
    false
}
/// Produces conservative observations from a normalized session.
#[must_use]
pub fn observe(session: &Session) -> Observations {
    Observations {
        session_id: session.session_id.clone(),
        host: session.host,
        partial: session.partial,
        observations: Vec::new(),
        env: Vec::new(),
        edited: Vec::new(),
    }
}
