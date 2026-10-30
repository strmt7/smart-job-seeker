# Custom build directory packaging check — 2026-10-30

On Windows, `build-windows.ps1 -StaticCrt` was run with `CARGO_TARGET_DIR` set to `target/crt-static-evaluation` and the existing local OpenSSL developer distribution. Cargo reported `target/crt-static-evaluation/debug/waypoint_web.exe`; the helper placed its OpenSSL DLL and build receipt in that directory. The receipt matched the SHA-256 of both files.

The packager was then run with `--build-dir target/crt-static-evaluation/debug`, the existing pinned Python package and PDF wheels, and `--no-libreoffice`. It created `target/custom-build-package-check-20261030.zip`, containing 167 dependency license inventories and 1,574 inventoried Python files. The 37,441,004-byte ZIP's executable and OpenSSL DLL hashes matched both its package manifest and build receipt. This used local package inputs; it did not exercise automatic downloading.

LibreOffice was intentionally omitted, so this bundle needs separate converter setup and is not a complete installation. The ZIP was not installed or run on a clean Windows host. The requested static CRT option is recorded, but this packaging check did not inspect imports or establish that all bundled components have no runtime prerequisites. No applications were sent.
