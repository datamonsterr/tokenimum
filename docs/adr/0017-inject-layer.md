# ADR 0017: Inject layer

Date: 2026-10-29. Status: accepted (epic E4.4).

1. **Candidates** for a prompt: active facts of the repository that pass the fire gate and the host
   scope (`Lifecycle::injectable`, text from `facts::render::claim`); pointer lines (`- ...` bullets,
   at most 20 per file) from `MEMORY.md` in the repository root, the cwd, the agent home and
   `<agent home>/projects/<slug>/memory/` when `inject.memory` is true; skill candidates from the
   `SkillSource` trait (default: none).
2. **Only new candidates per session.** Ids already injected in the session are skipped, so a long
   session does not repeat the block on every prompt.
3. **Scoring.** One `Purpose::Relevance` request with a score question `rel_<id>` per candidate,
   bounded by `relevance_timeout_ms` (default 1,200 ms: the 1,500 ms prompt read timeout minus a
   margin). The prompt text in the state is redacted. `tkm_core::inject::select` applies the 1.5
   threshold (1.0 for fire-gated facts), dedupe and the token budget.
4. **Fallback.** On timeout, `NoDecision` or no key the block is the cached default block (top
   candidates by order, no per-prompt scoring). Timeouts and failures write one `errors.log` line.
5. **Cache.** `inject::Cache`, key = (prompt hash, candidate-set hash), 10 minutes, process-wide.
6. **Deferral.** If the adapter renders the host's no-op for an inject (Cursor's
   `beforeSubmitPrompt`, Codewhale `session_start`), the block is kept in `pending_inject` and goes
   out with the next injectable event of the session: `session_start`, `tool_done` or `tool_before`.
   The same mechanism carries redirects the host could not show.
