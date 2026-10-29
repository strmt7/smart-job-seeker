# Waypoint local web prototype

On Windows, build the prototype from the repository root with:

```powershell
./apps/web_app/scripts/build-windows.ps1
```

The helper requires Rust, the MSVC build tools and a compatible x64 OpenSSL developer installation (headers, import library and runtime DLL). It checks the standard `Program Files/OpenSSL-Win64` and `Program Files/OpenSSL` locations; pass `-OpenSslRoot` or set `OPENSSL_DIR` to use another installation. It builds one job at a time by default and copies the OpenSSL runtime next to `target/debug/waypoint_web.exe`. Add `-Release` for a release build. This is a build helper, not an installer or a dependency downloader.

Use `./apps/web_app/scripts/build-windows.ps1 -Test` to run the web-app tests or `./apps/web_app/scripts/build-windows.ps1 -Clippy` to lint its targets with warnings treated as errors. Both commands configure the same detected OpenSSL paths as the build.

To run a specific ignored integration test, use `-Test -Ignored -TestFilter <test-name>`. Inspect its prerequisites first: ignored tests can start local browsers, access the network or require installed tools.

For development tests, use `./apps/web_app/scripts/build-windows.ps1 -Test`; add `-TestFilter <name>` to run one test. The helper configures the same OpenSSL environment for tests as for builds.

Start the resulting executable to open the local browser workspace. On Windows, use the notification-area menu to reopen the workspace or exit; double-click the icon to reopen it. The Windows default build uses SQLCipher for the workspace database with a key protected by the current Windows account's DPAPI. When an existing plaintext workspace is migrated, the original database is deliberately retained and the interface reports that it remains on disk; protect it as private data. Application-record folders remain readable as requested. The prototype does not send applications from its interface.

The executable and its OpenSSL DLL must be distributed together. Installation, portable dependency provisioning, backup and key recovery are not yet complete. See `docs/WEB_PROTOTYPE_CHECKPOINT.md` for current capabilities and limitations.

The project uses free local tools and must not require paid APIs. Wider-web search uses a user-configured SearXNG-compatible provider; use a free or self-hosted instance. The app cannot determine an independent provider's pricing or guarantee its availability. Local-model features use Ollama rather than a paid inference API.

To create a local prototype ZIP after a build, use Python 3.11 or newer:

```powershell
./apps/web_app/scripts/build-windows.ps1 -Release
python ./apps/web_app/scripts/package-windows.py --release --output ./prototype-local/waypoint-release.zip
```

The output path must be new and inside the repository. The packaging step checks the build receipt, collects local license files for the Windows runtime dependency graph, and downloads a pinned Python 3.12 x64 runtime into the bundle. For offline packaging, provide the matching NuGet archive with `--python-package <path>` and the pinned PDF wheels with `--wheel-dir <path>`. Their SHA-256 digests are checked before inclusion. Extract the ZIP and run `install.ps1` to copy it into a new per-user folder and create a Start Menu shortcut, or run the executable in the extracted `app` folder. The installer checks that the executable, bundled OpenSSL DLL and inventoried Python runtime files match the package manifest before creating the installation folder. These checks detect missing or damaged files; the unsigned manifest does not establish package authenticity. Existing installation folders and shortcuts are not overwritten. The bundled Python includes the pinned PDF libraries, so PDF features can use it without a separate setup download. The package is an unsigned prototype; Microsoft Visual C++ runtime, a Chromium browser and Ollama are not installed by it, and document conversion requires a separately supplied LibreOffice distribution. The dependency license inventory still needs review before redistribution.

For an optional private document converter, pass `--libreoffice-dir <directory>` pointing to a complete LibreOffice distribution with `program/soffice.com` and its license. The distribution is copied into the package and its files are inventoried and checked by the installer. This can substantially increase package size. It does not download LibreOffice, and a real bundled conversion has not yet been qualified.
