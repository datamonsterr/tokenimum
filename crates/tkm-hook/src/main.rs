#![forbid(unsafe_code)]
//! `tkm-hook` binary entry point (E4.1).
//!
//! The hook binary is registered by host installers as:
//! `tkm-hook --host <host> --event <event>`
//!
//! All logic lives in `lib.rs`; `main` is intentionally thin.

use std::io::{self, Read, Write};

use tkm_config::paths::Paths;
use tkm_hook::{parse_host_and_event, run_hook_with_stdin, run_with_panic_guard};
use tkm_proto::Host;

fn main() {
    let raw_args: Vec<String> = std::env::args().skip(1).collect();
    let (parsed_host, _) = parse_host_and_event(&raw_args);
    let fallback_host = parsed_host.unwrap_or(Host::Claude);

    let mut stdout = io::stdout();
    run_with_panic_guard(
        fallback_host,
        || {
            let Some(paths) = Paths::from_env() else {
                if let Some(host) = parsed_host {
                    let _ = io::stdout().write_all(host.noop_reply().as_bytes());
                } else {
                    let _ = io::stdout().write_all(b"");
                }
                let _ = io::stdout().flush();
                return;
            };

            let mut output_buf = Vec::new();
            run_hook_with_stdin(
                &raw_args,
                std::env::vars(),
                || {
                    let mut bytes = Vec::new();
                    let _ = io::stdin().read_to_end(&mut bytes);
                    bytes
                },
                Some(&paths.socket),
                Some(&paths.errors_log),
                &mut output_buf,
            );
            let _ = io::stdout().write_all(&output_buf);
            let _ = io::stdout().flush();
        },
        &mut stdout,
    );
}
