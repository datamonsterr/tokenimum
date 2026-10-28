# ADR 0001: Defaults for the SPEC open questions

Date: 2026-10-29. Status: accepted (implementation defaults; revisit on owner feedback).

SPEC section 17 lists nine open questions. Implementation proceeds with these defaults so no
epic blocks on them. Each can change later through a new ADR.

1. **TypeScript shims (opencode, Pi):** accepted as the one documented exception (SPEC 13.3).
   Shim mode is the default; `--no-shims` keeps both harnesses fold-only.
2. **DeepSeek harness:** Codewhale (formerly DeepSeek-TUI), as the spec assumes.
3. **SQLite and TLS:** accept `rusqlite` (bundled) and `rustls` with its crypto provider through
   `docs/unsafe-allowlist.md`. No pure-Rust store in this phase.
4. **otherskill hand-off:** tokenimum calls otherskill (`otherskill select --json`), one merged
   injection block when the person agrees during `tkm init` (SPEC 9.4).
5. **superorchestrator channel:** decided in epic E9 against the current superorchestrator CLI and
   MCP; `tkmd` runs standalone, not inside the superorchestrator supervisor.
6. **Port or re-implement TE:** port TokenEconomizer logic (Apache-2.0, attribution kept in file
   headers and `NOTICE`); re-implement all I/O in Rust.
7. **Agreement mode:** off by default; available per config and preset.
8. **To-verify items:** closed in epics E5A, E5B and E6 and recorded in `docs/verified/`.
   Unverified surfaces use the safe fallback (report-only pruning, idle-fold).
9. **Instructions-file injection:** only for untracked instruction files by default; committed
   `AGENTS.md` needs an explicit opt-in key `inject.instructions_file_tracked = true`.
