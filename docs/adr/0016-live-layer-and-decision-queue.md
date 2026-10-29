# ADR 0016: Live layer and decision queue

Date: 2026-10-29. Status: accepted (epic E4.3).

1. **One `TurnGuard` per turn.** A turn starts at `prompt` (or lazily at the first `tool_done`) and
   ends at `turn_end` or when the next prompt arrives with calls recorded (interrupted turn, never a
   good turn). The guard decides when to request a judge: it enforces "at most one in flight" and
   "at most one per 5 calls" itself, so the daemon does not duplicate those rules.
2. **Queue.** A judge request becomes a `JudgeJob` (a snapshot of the turn's calls) sent with
   `try_send` to the effect worker, which spawns one task per job. A full queue releases the guard's
   pending slot at once. The job builds the redacted digest (`tkm-decide::digest`), asks the three
   SPEC 7.4 questions in one request (`purpose = loop_guard`) and applies the outcome.
3. **Delivery.** A "stuck" verdict is stored inside the guard (`on_judge_verdict`); the guard hands
   it back as a redirect on a later `tool_done`. Jobs carry the turn sequence number; a verdict for a
   turn that ended (or was replaced) is dropped. A redirect that the host cannot show on this event
   stays pending for the current turn and goes out on the next injectable event; it is discarded
   when that turn ends. Prompt context deferred by a host is stored separately and survives the
   turn boundary until an injectable event.
4. **Shadow.** A decision is *acting* only if `loop.mode = redirect` and the answering model version
   has a calibration row (SPEC 8.5). Otherwise (mode `shadow`, or the balanced preset before
   calibration) the decision row is stored with `shadow = 1`, the guard's pending slot is released,
   and a would-be stuck verdict writes a `loop.shadow` event row. Nothing is injected. A backstop
   redirect (code rule) is shadowed the same way when `loop.mode = shadow`.
5. **Escalation.** A second stuck segment after `redirects_per_turn` makes the guard return
   `Escalate`. Always: a `loop.escalation` event row. Interactive: a notice naming the stop key
   (`Esc`). Delegated with an `Escalator` that `handles` the session: `Escalator::escalate` on the
   effect worker. Delegated without adapter (`TKM_DELEGATED` marker in the request env): the
   optional `loop.escalate_command` runs through `sh -c` with `TKM_HOST`, `TKM_SESSION`, `TKM_REPO`
   and `TKM_TURN_CALLS` set. In shadow mode only the event row is written.
6. **Envelope.** Good turns (`is_good_turn`) append to a per-(repo, host) `EnvelopeTracker`; the
   sample is queued for the `envelopes` ring. The value is cached in the session so `tool_done`
   never sorts samples. Without an envelope the guard never requests a judge.
7. **Failures.** `NoDecision` (except `Disabled`) writes one `errors.log` line; `CodeOnly` (no key,
   over budget) is silent. In both the guard falls back to code rules.

Open: agreement mode (`decide.agreement`) is not wired into the daemon's decider; single-model
decisions only (deferred).
