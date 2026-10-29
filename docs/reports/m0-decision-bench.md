# M0 decision bench

source: synthetic test double. Real per-model numbers need a recorded cache; run `tkm replay --bench --record fixtures/bench` with OPENROUTER_API_KEY set. Synthetic test doubles are not real model measurements.

50 sessions: 25 Claude Code, 25 Codex. Labels derive from interruption (stuck) or normal completion (progress). Each turn uses the production loop-guard questions and validated cached responses.

| Model | Turns | Agreement at 0.85 | Mean latency (ms) | Total cost (USD) | Fitted stuck threshold | precision | Recall |
|---|---:|---:|---:|---:|---:|---:|---:|
| synthetic/model-1-20261029 | 50 | 48/50 | 100 | $0.000200 | 0.91 | 0.963 | 1.000 |
| synthetic/model-2-20261029 | 50 | 48/50 | 150 | $0.000400 | 0.86 | 0.929 | 1.000 |

Fact filter: the first command is a task-specific, harmless candidate in each synthetic turn. Agreement is against those fixture labels using stable < 0.30 and safe >= 0.80.

| Model | stable agreement | safe agreement | Mean latency (ms) | Total cost (USD) |
|---|---:|---:|---:|---:|
| synthetic/model-1-20261029 | 50/50 | 50/50 | 100 | $0.000200 |
| synthetic/model-2-20261029 | 50/50 | 50/50 | 150 | $0.000400 |
