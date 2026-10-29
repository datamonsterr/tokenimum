//! Fold-only integration mode: instructions block, skill generation and run wrapper.
//!
//! Backs `--no-shims` and the fallback when `tkm doctor` finds hook events missing
//! (SPEC 6.12, ADR 0026). Everything is synchronous and performs no network I/O.

pub mod instructions;
pub mod run;
pub mod skill;

use tkm_proto::Host;

/// Integration level for a harness.
///
/// `Hooks` means the harness has native command hooks; `Shim` means a generated
/// TypeScript shim is used; `FoldOnly` means read-only log folding with optional
/// instructions-file injection and a skill.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IntegrationMode {
    /// Native command-hook integration (Claude, Codex, agy, Cursor, Kimi, Codewhale).
    Hooks,
    /// Generated TypeScript shim (opencode, Pi with `--shims`).
    Shim,
    /// Fold-only: no live hooks; idle-fold plus instructions file and skill (SPEC 6.12).
    FoldOnly,
}

/// Returns the best integration mode for `host`.
///
/// opencode and Pi use `Shim` when `shims` is `true`, otherwise `FoldOnly`.
/// All other harnesses always use `Hooks`.
#[must_use]
pub fn mode_for(host: Host, shims: bool) -> IntegrationMode {
    match host {
        Host::Opencode | Host::Pi => {
            if shims {
                IntegrationMode::Shim
            } else {
                IntegrationMode::FoldOnly
            }
        }
        _ => IntegrationMode::Hooks,
    }
}

/// Resolve a configured level with doctor health and the person's enable switch.
/// Unknown or unavailable integration paths degrade to fold-only.
#[must_use]
pub fn effective_mode(
    host: Host,
    configured: &str,
    enabled: bool,
    fallback: bool,
) -> IntegrationMode {
    if !enabled || fallback || configured == "fold-only" {
        return IntegrationMode::FoldOnly;
    }
    let native = mode_for(host, true);
    match configured {
        "auto" => native,
        "hooks" if native == IntegrationMode::Hooks => native,
        "shim" if native == IntegrationMode::Shim => native,
        _ => IntegrationMode::FoldOnly,
    }
}
