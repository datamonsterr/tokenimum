# ADR 0046: Performance gates

Status: In progress

SPEC section 14 requires absolute budgets and a 20% regression gate for every
path. The following inventory records the current enforcement and outstanding
work; incomplete entries are not evidence of a gate.

| Path | SPEC budget | Measurement and absolute check | CI job and relative comparison |
|---|---|---|---|
| `tkm-hook` start to exit, `tool_done` | p50 < 3 ms, p99 < 10 ms | hyperfine round trip; `check_hook_latency.py` | `bench`: round-trip p50 and p99 |
| `tkmd` handling of `tool_done` | < 200 µs | Criterion `tool_done`; `check_bench_budget.py` | `bench`: Criterion |
| `prompt` with relevance scoring | < 1,500 ms, cached reply < 5 ms | Pending | Pending |
| Shim overhead (opencode, Pi) | p50 < 15 ms, p99 < 40 ms | Pending | Pending |
| Fold of one session | < 500 ms CPU, excluding decisions | Pending | Pending |
| `tkmd` idle memory | < 30 MB (< 40 MB with web) | `/proc` VmRSS; `check_idle_memory.py` | `idle-memory`: default and web |
| `tkmd` idle CPU | ~0 | `/proc` process CPU ticks; `idle_metrics.py` | `idle-memory`: default and web |
| `tkm top` refresh | < 50 ms per frame | Criterion `render`; `check_bench_budget.py` | `bench`: Criterion |

Interpret the qualitative idle CPU budget as **less than 1% average CPU over a
30-second idle window** for each daemon variant. CPU is process user plus
system ticks divided by elapsed monotonic time and clock ticks per second.
Regression comparisons use the same recorded values and a 120% threshold;
they require a prior main-branch baseline. Tick quantization makes a zero
baseline possible and needs validation in CI.
