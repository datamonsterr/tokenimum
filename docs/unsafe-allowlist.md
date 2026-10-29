# Dependency unsafe allowlist

SPEC 13.2 permits dependency unsafe only with a justification and a review. The table records
candidate dependencies in the locked tree identified by inspecting published source and their
roles on 2026-10-29; this initial source triage is not a complete safety or maintenance audit.
CI scans each binary with cargo-geiger 0.14.0 and requires allowlist entries for crates with
used unsafe and crates that could not be fully scanned. Unknown file owners fail closed.
Allowlist entries absent from all reports produce a warning. Review maintenance (within 12 months),
unsafe use, and justification before adding a row; remove rows found not to use unsafe after the first successful CI scan. Geiger
has scanned these dependencies in CI; this list is not a complete safety or maintenance audit.
The count baseline must be generated
from the three CI `geiger-reports` JSON files using `python3 scripts/geiger_diff.py
--write-baseline docs/geiger-baseline.json tkmd-geiger.json tkm-cli-geiger.json
tkm-hook-geiger.json`, reviewed, and committed. Until then CI warns loudly and skips only the
count comparison. Overlapping reports and multiple versions are represented by the largest
observed count per crate name, without adding the same crate repeatedly.

| crate | justification | reviewed |
| --- | --- | --- |
| aho-corasick | Multi-pattern searching. | 2026-10-29 |
| allocator-api2 | Allocation interfaces. | 2026-10-29 |
| anstream | Terminal stream handling. | 2026-10-29 |
| anstyle | Terminal styling. | 2026-10-29 |
| anstyle-parse | Terminal escape parsing. | 2026-10-29 |
| anstyle-query | Terminal capability detection. | 2026-10-29 |
| atomic-waker | Asynchronous wakeup synchronization. | 2026-10-29 |
| aws-lc-rs | TLS cryptography provider bindings. | 2026-10-29 |
| aws-lc-sys | Bundled cryptographic C library bindings. | 2026-10-29 |
| base64 | Base64 encoding. | 2026-10-29 |
| bit-set | Bit set storage. | 2026-10-29 |
| bit-vec | Bit vector storage. | 2026-10-29 |
| bitflags | Bit flag representations. | 2026-10-29 |
| block-buffer | Block buffering for hash digests (sha2). | 2026-10-29 |
| bstr | Byte string operations. | 2026-10-29 |
| bytes | Reference-counted byte buffers. | 2026-10-29 |
| castaway | Type casting primitives. | 2026-10-29 |
| cc | Native dependency build tooling. | 2026-10-29 |
| clap | Command-line parsing interfaces. | 2026-10-29 |
| clap_builder | Command-line parsing. | 2026-10-29 |
| clap_derive | Command-line parser derive macro. | 2026-10-29 |
| clap_lex | Command-line lexing. | 2026-10-29 |
| compact_str | Compact string storage. | 2026-10-29 |
| cpufeatures | Runtime CPU feature detection. | 2026-10-29 |
| critical-section | Cross-platform critical-section primitives. | 2026-10-29 |
| crossterm | Terminal control and platform APIs. | 2026-10-29 |
| deranged | Range-checked numeric types used by time. | 2026-10-29 |
| derive_more-impl | Implementation of derive macros for common traits. | 2026-10-29 |
| dirs-sys | Platform directory interfaces. | 2026-10-29 |
| either | Sum type utilities. | 2026-10-29 |
| errno | Operating system error access. | 2026-10-29 |
| foldhash | Hashing implementation. | 2026-10-29 |
| form_urlencoded | URL form encoding. | 2026-10-29 |
| futures-channel | Asynchronous channels. | 2026-10-29 |
| futures-core | Asynchronous core interfaces. | 2026-10-29 |
| futures-task | Asynchronous task primitives. | 2026-10-29 |
| futures-util | Asynchronous combinators. | 2026-10-29 |
| generic-array | Fixed-size arrays for digest output (sha2). | 2026-10-29 |
| getrandom | Operating system randomness. | 2026-10-29 |
| globset | Glob matching. | 2026-10-29 |
| hashbrown | Hash table implementation. | 2026-10-29 |
| hashlink | Linked hash map storage. | 2026-10-29 |
| http | HTTP data structures. | 2026-10-29 |
| http-body | HTTP body buffer interfaces. | 2026-10-29 |
| http-body-util | HTTP body combinators. | 2026-10-29 |
| httparse | HTTP parsing. | 2026-10-29 |
| hyper | HTTP transport. | 2026-10-29 |
| hyper-util | HTTP transport utilities. | 2026-10-29 |
| icu_collections | Unicode data representations. | 2026-10-29 |
| icu_locale_core | Locale data representations. | 2026-10-29 |
| icu_normalizer | Unicode normalization. | 2026-10-29 |
| icu_normalizer_data | Unicode normalization data. | 2026-10-29 |
| icu_properties_data | Unicode character property data. | 2026-10-29 |
| icu_provider | Unicode data loading. | 2026-10-29 |
| idna | International domain name handling. | 2026-10-29 |
| indexmap | Ordered hash maps. | 2026-10-29 |
| inotify | Linux filesystem event interface. | 2026-10-29 |
| inotify-sys | Linux filesystem event bindings. | 2026-10-29 |
| itertools | Iterator utility functions. | 2026-10-29 |
| itoa | Integer formatting. | 2026-10-29 |
| lazy_static | Lazy initialization support. | 2026-10-29 |
| libc | Operating system FFI declarations. | 2026-10-29 |
| libsqlite3-sys | Bundled SQLite and FFI bindings. | 2026-10-29 |
| linux-raw-sys | Linux system call bindings. | 2026-10-29 |
| lock_api | Low-level lock interfaces. | 2026-10-29 |
| log | Logging interfaces. | 2026-10-29 |
| lru | Least recently used cache. | 2026-10-29 |
| memchr | Optimized byte searching. | 2026-10-29 |
| mio | Operating system event polling. | 2026-10-29 |
| notify | Filesystem watching. | 2026-10-29 |
| nu-ansi-term | Terminal colors. | 2026-10-29 |
| num-traits | Numeric traits. | 2026-10-29 |
| once_cell | One-time initialization primitives. | 2026-10-29 |
| openssl-probe | Discovery of system TLS certificate locations. | 2026-10-29 |
| parking_lot | Synchronization primitives. | 2026-10-29 |
| parking_lot_core | Platform parking primitives. | 2026-10-29 |
| percent-encoding | URL percent encoding. | 2026-10-29 |
| pin-project-lite | Pinned future projection. | 2026-10-29 |
| potential_utf | Unicode data representation. | 2026-10-29 |
| powerfmt | Formatting utilities used by time. | 2026-10-29 |
| proc-macro2 | Compiler token interoperation. | 2026-10-29 |
| proptest | Property-based testing. | 2026-10-29 |
| rand | Random number generation. | 2026-10-29 |
| rand_core | Random generator interfaces. | 2026-10-29 |
| ratatui-core | Terminal UI core. | 2026-10-29 |
| regex | Regular expression matching. | 2026-10-29 |
| regex-automata | Regular expression automata. | 2026-10-29 |
| reqwest | HTTP client and TLS integration. | 2026-10-29 |
| rusqlite | SQLite safe interface over FFI. | 2026-10-29 |
| rustix | Operating system interfaces. | 2026-10-29 |
| rustls | TLS implementation and cryptographic interfaces. | 2026-10-29 |
| rustls-pki-types | TLS public key representations. | 2026-10-29 |
| rustls-platform-verifier | Platform certificate verification. | 2026-10-29 |
| rustls-webpki | TLS certificate verification. | 2026-10-29 |
| rustversion | Compiler version detection and conditional compilation. | 2026-10-29 |
| ruzstd | Pure-Rust zstd decoding of old Codex rollouts. | 2026-10-29 |
| ryu | Floating-point formatting. | 2026-10-29 |
| same-file | Filesystem identity checks. | 2026-10-29 |
| scopeguard | Cleanup guard implementation. | 2026-10-29 |
| serde | Serialization framework. | 2026-10-29 |
| serde_core | Serialization core. | 2026-10-29 |
| serde_json | JSON parsing and serialization. | 2026-10-29 |
| sharded-slab | Concurrent sharded slab storage. | 2026-10-29 |
| sha2 | SHA-256 for the shim content hash (E6); SIMD backends. | 2026-10-29 |
| signal-hook | Signal handling. | 2026-10-29 |
| signal-hook-mio | Signal event integration. | 2026-10-29 |
| signal-hook-registry | Signal handler registration. | 2026-10-29 |
| slab | Indexed storage. | 2026-10-29 |
| smallvec | Inline storage and initialization. | 2026-10-29 |
| socket2 | Socket system calls. | 2026-10-29 |
| stable_deref_trait | Trait for types with stable dereference targets. | 2026-10-29 |
| static_assertions | Compile-time assertions. | 2026-10-29 |
| subtle | Constant-time operations. | 2026-10-29 |
| syn | Build-time proc-macro dependency; Geiger can report unscanned compiler-used files. | 2026-10-29 |
| sync_wrapper | Synchronization wrapper. | 2026-10-29 |
| tempfile | Temporary file handling. | 2026-10-29 |
| thiserror | Error type derive support. | 2026-10-29 |
| thread_local | Thread-local storage. | 2026-10-29 |
| time | Date and time representations. | 2026-10-29 |
| tinystr | Compact Unicode strings. | 2026-10-29 |
| tokio | Asynchronous runtime and operating system I/O. | 2026-10-29 |
| toml_edit | TOML editing. | 2026-10-29 |
| toml_parser | TOML parsing. | 2026-10-29 |
| tower-http | HTTP middleware. | 2026-10-29 |
| tracing | Event instrumentation. | 2026-10-29 |
| tracing-core | Event instrumentation core. | 2026-10-29 |
| tracing-log | Logging and tracing integration. | 2026-10-29 |
| tracing-subscriber | Instrumentation collection. | 2026-10-29 |
| try-lock | Nonblocking lock primitives. | 2026-10-29 |
| unarray | Property-based array support. | 2026-10-29 |
| unicode-ident | Unicode identifier handling. | 2026-10-29 |
| unicode-width | Unicode terminal width calculation. | 2026-10-29 |
| url | URL parsing. | 2026-10-29 |
| utf8_iter | UTF-8 iteration. | 2026-10-29 |
| utf8parse | UTF-8 parser. | 2026-10-29 |
| wait-timeout | Process timeout handling. | 2026-10-29 |
| winnow | Parser primitives. | 2026-10-29 |
| writeable | Unicode output formatting. | 2026-10-29 |
| yoke | Zero-copy data ownership. | 2026-10-29 |
| zeroize | Sensitive data clearing. | 2026-10-29 |
| zerotrie | Zero-copy trie representation. | 2026-10-29 |
| zerovec | Zero-copy vector representation. | 2026-10-29 |
| zmij | Floating-point formatting. | 2026-10-29 |
