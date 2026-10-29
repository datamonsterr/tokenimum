# E10 bench API verification

Checked 2026-10-29: [clap derive reference](https://docs.rs/clap/latest/clap/_derive/) documents boolean and optional path arguments; [sha2 0.10.9](https://docs.rs/sha2/0.10.9/sha2/) documents the one-shot SHA-256 API. The typed decision response and validation contract was checked against the OpenRouter reference in [E3 verification](e3-openrouter-decisions.md).

Also checked 2026-10-29: [OpenRouter Decisions request](https://openrouter.ai/docs/api/api-reference/alphadecisions/submit-a-decisions-request) documents the model, state and typed questions; [Tokio timeout](https://docs.rs/tokio/latest/tokio/time/fn.timeout.html) documents cancellation and the enabled timer requirement.
