// Portions ported from microsoft/TokenEconomizer (tokeneconomizer/loopguard/rules.py; tokeneconomizer/plugin/hosts.py, commit d0675a3),
// Copyright (c) Microsoft Corporation, licensed under the Apache License, Version 2.0.
//! Code-owned redirect templates and host notice hints (SPEC 7.4, 11.5).
use crate::model::Host;
use serde::{Deserialize, Serialize};
use std::str::FromStr;
/// Exactly the seven decision choices in SPEC 7.4.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RedirectKind {
    /// Redirect choice `reread_error`.
    RereadError,
    /// Redirect choice `check_env`.
    CheckEnv,
    /// Redirect choice `smaller_step`.
    SmallerStep,
    /// Redirect choice `revert_last_edit`.
    RevertLastEdit,
    /// Redirect choice `different_test`.
    DifferentTest,
    /// Redirect choice `read_more_code`.
    ReadMoreCode,
    /// Redirect choice `ask_user`.
    AskUser,
}
impl RedirectKind {
    /// Stable choice identifier.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::RereadError => "reread_error",
            Self::CheckEnv => "check_env",
            Self::SmallerStep => "smaller_step",
            Self::RevertLastEdit => "revert_last_edit",
            Self::DifferentTest => "different_test",
            Self::ReadMoreCode => "read_more_code",
            Self::AskUser => "ask_user",
        }
    }
    /// Render a code template with a validated command form and error class only.
    /// Invalid evidence uses fixed placeholders; output prose is never copied.
    #[must_use]
    pub fn render(self, params: &RedirectParams<'_>) -> String {
        let command = command_evidence(params.failing_command);
        let error = error_evidence(params.error_line);
        let instruction = match self {
            Self::RereadError => {
                "Re-read the error and the failing expectation before editing again."
            }
            Self::CheckEnv => {
                "Check the environment (imports, installed packages, working directory) before editing again."
            }
            Self::SmallerStep => {
                "Try a smaller step that isolates one hypothesis before editing again."
            }
            Self::RevertLastEdit => {
                "Revert the last edit if appropriate and compare the failure before trying another change."
            }
            Self::DifferentTest => "Try a different test that narrows the cause of this failure.",
            Self::ReadMoreCode => "Read the surrounding code and its callers before editing again.",
            Self::AskUser => {
                "Ask the user for the missing context or constraint needed to make progress."
            }
        };
        format!(
            "tokenimum: the last {} calls repeated \"{error}\". {instruction} Failing command: \"{command}\".",
            params.calls
        )
    }
    /// Rule-only fallback, requiring no model choice.
    #[must_use]
    pub const fn rule_default() -> Self {
        Self::RereadError
    }
}
/// Unknown redirect name.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("unknown redirect kind: {0}")]
pub struct ParseRedirectKindError(
    /// Unrecognized choice identifier.
    pub String,
);
impl FromStr for RedirectKind {
    type Err = ParseRedirectKindError;
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "reread_error" => Ok(Self::RereadError),
            "check_env" => Ok(Self::CheckEnv),
            "smaller_step" => Ok(Self::SmallerStep),
            "revert_last_edit" => Ok(Self::RevertLastEdit),
            "different_test" => Ok(Self::DifferentTest),
            "read_more_code" => Ok(Self::ReadMoreCode),
            "ask_user" => Ok(Self::AskUser),
            _ => Err(ParseRedirectKindError(value.into())),
        }
    }
}
/// Untrusted evidence rendered inside a fixed template.
#[derive(Debug, Clone, Copy)]
pub struct RedirectParams<'a> {
    /// Calls in the stuck segment.
    pub calls: u32,
    /// Last command; only ASCII command-form characters within 160 bytes are accepted.
    pub failing_command: &'a str,
    /// First error line; only its leading error-class identifier is rendered.
    pub error_line: &'a str,
}
/// Interactive stop key, for a notice only; never sent as a keypress.
/// Claude: TE `plugin/hosts.py` `Claude.interrupt` (2.1.306).
/// Codex: TE `Codex.interrupt` (0.163.0; also mentions /stop for background commands).
/// Pi: TE `Pi.interrupt`. Agy, Cursor, Kimi, Deepseek and Opencode: the task's
/// SPEC 7.4 extension uses Esc pending host integration verification.
#[must_use]
pub const fn stop_key(_host: Host) -> &'static str {
    "Esc"
}

pub(super) fn command_evidence(text: &str) -> String {
    let valid = !text.is_empty()
        && text.len() <= 160
        && text
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"_./:@%+=, -".contains(&c));
    if valid
        && text
            .trim()
            .bytes()
            .next()
            .is_some_and(|c| c.is_ascii_alphanumeric() || b"_./".contains(&c))
    {
        text.trim().to_owned()
    } else {
        "the previous command".into()
    }
}

pub(super) fn error_evidence(text: &str) -> String {
    // Keep an identifier, never the message after ':'. Stripping delimiters from
    // prose would still preserve instructions an attacker placed in that prose.
    pattern!(r"\A((?:[A-Za-z][A-Za-z0-9_]{0,53})?(?:Error|Exception))(?::|\z)")
        .and_then(|re| re.captures(text))
        .and_then(|c| c.get(1).map(|m| m.as_str().to_owned()))
        .unwrap_or_else(|| "the same error".into())
}
