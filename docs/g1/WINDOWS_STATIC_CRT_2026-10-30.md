# Windows runtime experiment — 2026-10-30

This is a development measurement, not a clean-host installation qualification.
At source commit `6a8e90b7c90951ea9d1dc2142bc61e98f755b75b`, a Windows x64
debug build used Rust 1.99.0 and Cargo 1.99.0 with
`RUSTFLAGS='-C target-feature=+crt-static'`, one Cargo job and a separate
`target/crt-static-evaluation` directory. The executable SHA-256 was
`9cf642d6a8a11a30c7178f8123229d05f551533c802cb33c0092c65b8c97b90b`.
MSVC `dumpbin /DEPENDENTS` did not list `VCRUNTIME140.dll` or the universal
CRT API-set DLLs as direct imports of this executable. The ordinary debug
executable on the same host did list them. This is an import-table observation,
not proof that all runtime dependencies are absent.

The OpenSSL 4.1.0 `libcrypto-4-x64.dll` used in the build had SHA-256
`257780926745efaa85b36b37ea3f2326d66205dd4625b4d6210b983dab2b523c`.
Its import table included universal CRT API-set DLLs. A focused SQLCipher
test with the same static-CRT flags passed: it migrated a fictional workspace,
checked that the database was encrypted, and reopened it. The full web app,
release build, installation on a clean host, Python and LibreOffice runtime
dependencies, and other Windows versions were not tested in this experiment.

The build helper's optional `-StaticCrt` requests this mode; the build receipt
and package manifest record the request, not a verified binary property. The
normal build remains unchanged. A Windows installation may still need system
runtime components, a browser and Ollama.

Research used the isolated `crawl4ai 0.9.4+superzip.portable2` provider with
robots checking, no external model, and 30-second page timeouts. The native
results were retained in the ignored local `target` research directory:

- [Microsoft's redistribution guidance](https://learn.microsoft.com/en-us/cpp/windows/redistributing-visual-cpp-files?view=msvc-170), HTTP 200, 447,534 bytes, SHA-256 `1d22bc82dccda9e587deef115cf3706e49fd6e2ec519efc08220b451d7d19a1f`, saved as `target/crawl4ai-msvc-redistribution-canonical-20261030.json`. It describes licensing restrictions and notes that app-local runtime deployment has servicing drawbacks.
- [Chrome for Testing documentation](https://developer.chrome.com/docs/automation-and-testing/chrome-for-testing), HTTP 200, 348,497 bytes, SHA-256 `6d49b208288eafabc2b3478334f6dafda02b6feaa9d20546cc6bad9c7520b95e`, saved as `target/crawl4ai-chrome-for-testing-canonical-20261030.json`. Its stated trusted-content limitation makes it an unsuitable default for browsing arbitrary job sites. No browser binary was added to the package.
