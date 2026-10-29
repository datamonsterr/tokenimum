# ADR 0011: Decision client defaults left open by SPEC 8

Date: 2026-10-29. Status: accepted (epic E3).

1. **Live API wins over SPEC 8.1 wording.** Answers are typed objects (`noul`, `choice`,
   `score` fields, see `docs/verified/e3-openrouter-decisions.md`). A `score` answer is valid
   within 0 to n-1; every probability is finite and within 0–1.
2. **No persistence or config file I/O in `tkm-decide`.** Spend rows, calibration rows and the
   cache are reached through traits (`SpendLedger`, `CalibrationStore`) and plain config structs.
   E4 implements them on `tkm-store` / `tkm-config`. The response cache is in memory.
3. **Failure never reaches the caller as an error.** The client returns an outcome:
   a decision, "no decision" (every model failed, disabled, over-size state) or "code only"
   (budget exhausted, no key). Non-retryable request errors (400, 401, 402, 403, 413) also end the
   chain as "no decision" without trying other models, since another model cannot fix them.
   429 counts as a model failure and moves on, like 5xx.
4. **Paths in digests.** Paths inside the repository become repository-relative. Other absolute
   paths are replaced by `[outside-repo]`; any remaining home prefix becomes `~`.
5. **Shadow window.** Shadow mode ends when both 14 days have passed and 200 turns were judged
   (the later of the two), so calibration always has enough labels.
6. **Calibration fit.** Each threshold is fitted alone over candidate cut points: best precision
   subject to a minimum recall (default 0.5); ties go to higher recall. The default precision
   target for `stuck` is 0.9. With no candidate meeting the recall floor, the model stays
   uncalibrated (log only).
7. **Free model slot.** `inception/mercury-decide:free` has no dated slug; it is the only undated
   chain entry allowed, and it is inactive until a re-probe answers.
