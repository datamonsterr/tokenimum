# ADR 0046: Performance gates

Status: Accepted

SPEC section 14 requires absolute budgets and a 20% regression gate for every
path. The following inventory maps each budget to its absolute check and regression
series. Jobs now default to standard GitHub-hosted Linux runners.

| Path | SPEC budget | Measurement and absolute check | CI job and relative comparison |
|---|---|---|---|
| `tkm-hook` start to exit, `tool_done` | p50 < 3 ms, p99 < 10 ms | hyperfine round trip; `check_hook_latency.py` | `bench`: round-trip p50 and p99 |
| `tkmd` handling of `tool_done` | < 200 µs | Criterion `tool_done`; `check_bench_budget.py` | `bench`: Criterion |
| `prompt` with relevance scoring | < 1,500 ms, cached reply < 5 ms | Criterion `prompt/scored` and `prompt/cached`; `check_bench_budget.py` | `bench`: Criterion |
| Shim overhead (opencode, Pi) | p50 < 15 ms, p99 < 40 ms | hyperfine `bench_shim.ts`; `shim_benchmark.py` checks both hosts | `bench`: shim p50 and p99 for each host |
| Fold of one session | < 500 ms CPU, excluding decisions | Criterion `fold_session` reads and folds Claude and Pi fixtures; `check_bench_budget.py` | `bench`: Criterion |
| `tkmd` idle memory | < 30 MB (< 40 MB with web) | `/proc` VmRSS; `check_idle_memory.py` | `idle-memory`: default and web |
| `tkmd` idle CPU | ~0 | `/proc` process CPU ticks; `idle_metrics.py` | `idle-memory`: default and web |
| `tkm top` refresh | < 50 ms per frame | Criterion `render`; `check_bench_budget.py` | `bench`: Criterion |

Interpret the qualitative idle CPU budget as **less than 1% average CPU over a
30-second idle window** for each daemon variant. CPU is process user plus
system ticks divided by elapsed monotonic time and clock ticks per second.
The absolute CPU check uses the raw measurement. For the relative comparison,
clamp values to one clock tick per 30 seconds; zero and one observed tick are
therefore indistinguishable, and a zero baseline cannot cause division by zero.
Both variants have independent series. Regression comparisons use a 120%
threshold and require a prior main-branch baseline; the new series become
effective after their first main run.

The prompt benchmark exercises candidate loading, relevance batching, selection,
rendering and caching, using an immediate deterministic decision response so CI
does not depend on an external service. It asserts that the scored path calls
the decider and that a subsequent session receives a nonempty cached block
without another call. Network latency is outside this deterministic measurement.
The daemon independently limits the wait for scoring to 1,200 ms by default.

The fold benchmark includes the fixture reader and the daemon's decisions-free
fold computations. Both run synchronously on one thread, so elapsed wall time
is a conservative upper bound on CPU time; disk waiting also counts. The shim
benchmark includes Bun startup, the real shim callback, and the hook round trip,
so its wall time conservatively includes more than shim overhead alone. It runs
the callbacks with small synthetic tool events against a running daemon.
SPEC-table synchronization is checked by `scripts/tests/test_perf_budget_table.py`
in the `bench` job.
