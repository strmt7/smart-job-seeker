# Windows static CRT release evaluation — 2026-11-01

On one Windows development host, source commit `fe1e25ba5cfd466b18b02e25727f78be5296cb07` was built with the pinned OpenSSL 4.1.0 development cache, one Cargo build job, a separate target directory, `-Release`, and `-StaticCrt`. The subsequent build using the new release default reused the same artifact and recorded `static_crt_requested: true`.

- Executable size: 15,532,544 bytes.
- Executable SHA-256: `b3289c74117e2a22d573aa3848e0d863e21a7257d79ec72f954e556049257db8`.
- Cargo.lock SHA-256: `8294607fc3a615ec28d3303c94725c3447e42d9e28c36e9d5aec6d9306dd7ecb`.
- MSVC `dumpbin /DEPENDENTS` showed no direct VCRUNTIME or UCRT API-set import in the executable. The adjacent `libcrypto-4-x64.dll` did import UCRT API-set DLLs. This inspection is not a complete transitive dependency audit.
- With a temporary isolated user workspace, the executable started and its loopback interface returned HTTP 200. The resulting database header was not the plaintext SQLite header. This is a startup check, not an assessment of encryption strength.
- The build-helper fixture tests passed on Windows PowerShell 5.1 and PowerShell 7, including the default release setting, explicit opt-out, and restoration of caller flags.

These results do not establish clean-host installation, complete runtime independence, or the behavior of bundled third-party components. Browser provisioning and the other installation limitations remain open.
