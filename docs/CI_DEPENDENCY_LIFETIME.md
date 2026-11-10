# Native SDK lifetime in hosted checks

Checkpoint: 2026-11-10. CI and coverage now provision the pinned OpenSSL developer
files under the runner's own temporary dependency directory, outside Cargo's
`target` tree. No host OpenSSL installation is assumed or changed. The same
149-file verification and environment setup remain in use.

The build-and-test job for `275aadc4` validated the SDK, restored a partial Rust
cache match, then failed compiling SQLCipher because `openssl/crypto.h` no longer
existed at its configured path. The pinned cache action calls `cleanTargetDir`
on a partial match. That function recursively traverses non-profile directories
and deletes ordinary files. A developer SDK inside `target` therefore had the
wrong lifetime, even though it was successfully downloaded and verified.

The relevant upstream implementation was read through pinned native source API
captures: [restore](https://github.com/Swatinem/rust-cache/blob/d1527c8f67ef9af357290475d4af4b0fab8f12e0/src/restore.ts)
and [cleanup](https://github.com/Swatinem/rust-cache/blob/d1527c8f67ef9af357290475d4af4b0fab8f12e0/src/cleanup.ts).
The cleanup source blob is `ac84d04021f796f885b818b0d10b6852fd41422a`.
No third-party implementation was copied into this repository.

This differs from the earlier local test-command error, which pointed at the
wrong library subdirectory. Here the configured include path was correct; cache
cleanup removed its contents. Installing another system SDK, suppressing the
compiler error, repeating downloads before the same cleanup, or weakening the
cache's integrity checks would not fix this ownership problem.

`tools/tests/test_ci_dependency_lifetime.py` executes the actual PowerShell setup
blocks from both workflow files in isolated fixture workspaces. Only provisioning
is replaced with a fictional SDK. The test then deletes its own Cargo output
and requires the selected headers to remain byte-identical in runner-owned
storage. Both methods failed against the old workflows and passed after the
change. The entire 24-test development-tool suite passed. These tests do not
install software, write the real GitHub environment files or touch another
workspace's Cargo output. They skip explicitly when PowerShell is unavailable.

The original `275aadc4` coverage run passed and uploaded its measured report;
CodeQL, agent-tool contracts, PDF regressions and supply-chain checks also passed.
Its build-and-test result remains a failure in the historical run. The follow-up
hosted runs must independently confirm the corrected environment; the Actions
results are authoritative for their exact commit. This correction changes no
application code, runtime dependency version, authorization rule or release gate.
No release was created.
