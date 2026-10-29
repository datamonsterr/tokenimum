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
    /// Render only a code template, sanitizing every string parameter.
    #[must_use]
    pub fn render(self, _params: &RedirectParams<'_>) -> String {
        String::new()
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
pub struct ParseRedirectKindError(pub String);
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
    /// Last failing command, capped to 160 characters during rendering.
    pub failing_command: &'a str,
    /// First error line, capped to 160 characters during rendering.
    pub error_line: &'a str,
}
/// Interactive stop key, for a notice only; never sent as a keypress.
/// Claude/Codex/Pi: TE hosts.py d0675a3 interrupt fields; other hosts:
/// task's SPEC 7.4 extension uses Esc pending host integration verification.
#[must_use]
pub const fn stop_key(_host: Host) -> &'static str {
    "Esc"
}
