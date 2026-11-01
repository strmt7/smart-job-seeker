# Workspace test checkpoint — 2026-11-01

On Windows, checkout `15b61ff`, `cargo test --workspace -- --test-threads=1`
passed with one Cargo build job. Ignored tests were not included. The existing
OpenSSL installation in `target/waypoint-openssl-4.1.0` was explicitly supplied
through `OPENSSL_DIR`, `OPENSSL_INCLUDE_DIR`, `OPENSSL_LIB_DIR` and `PATH` for
SQLCipher. An initial invocation without those variables failed to build
`libsqlite3-sys` because `OPENSSL_DIR` was missing; the configured run passed.

The separately run `cargo test -p waypoint_browser --test cdp_live -- --ignored
--test-threads=1` passed all 58 browser integration tests against fictional
local fixtures. `cargo fmt --all -- --check` passed. The browser crate's
`cargo clippy -p waypoint_browser --all-targets -- -D warnings` also passed in
the preceding staged-form change. A workspace-wide Clippy run was stopped
during recompilation of unchanged native dependencies and has no result.

These results do not include ignored live-model, Gmail, third-party-site or
installer tests, and do not establish end-to-end application readiness. No real
application was submitted.
