# Waypoint local web prototype

On Windows, build the prototype from the repository root with:

```powershell
./apps/web_app/scripts/build-windows.ps1
```

The helper requires Rust and the MSVC build tools. It checks for a compatible x64 OpenSSL developer installation in the standard `Program Files/OpenSSL-Win64` and `Program Files/OpenSSL` locations; pass `-OpenSslRoot` or set `OPENSSL_DIR` to use another installation. If none is available, it downloads the pinned, signed OpenSSL 4.1.0 x64 MSI, verifies it and extracts the development files into `target/waypoint-openssl-4.1.0` without installing OpenSSL system-wide. Provisioning needs network access and at least 1 GiB of free disk space; the download is about 58 MiB. For offline setup, pass `-OpenSslInstaller <path>` with that exact MSI. A verified existing cache is reused. Incomplete or modified caches are rejected and retained for inspection. The helper builds one job at a time by default and copies the OpenSSL runtime next to `target/debug/waypoint_web.exe`. Add `-Release` for a release build. Rust and MSVC are not installed by this helper.

When a later build uses a different OpenSSL DLL with the same name, the helper replaces it only if the previous Waypoint build receipt identifies the existing file. It keeps the old DLL in `target/<profile>/waypoint-openssl-backups`. An unrecognized DLL is left untouched.

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

The output path must be new and inside the repository. The packaging step checks the build receipt, collects local license files for the Windows runtime dependency graph, and downloads a pinned Python 3.12 x64 runtime into the bundle. For offline packaging, provide the matching NuGet archive with `--python-package <path>` and the pinned PDF wheels with `--wheel-dir <path>`. Their SHA-256 digests are checked before inclusion. Extract the ZIP and run `install.ps1` to copy it into a new per-user folder and create a Start Menu shortcut, or run the executable in the extracted `app` folder. The installer checks that the executable, bundled OpenSSL DLL and inventoried Python runtime files match the package manifest before creating the installation folder. These checks detect missing or damaged files; the unsigned manifest does not establish package authenticity. Existing installation folders and shortcuts are not overwritten. The bundled Python includes the pinned PDF libraries, so PDF features can use it without a separate setup download. The package is an unsigned prototype; Microsoft Visual C++ runtime, a Chromium browser and Ollama are not installed by it, and document conversion requires LibreOffice to be bundled or separately installed. The dependency license inventory still needs review before redistribution.

Packaging uses an installed LibreOffice distribution if available. Otherwise, on Windows, it downloads the pinned official LibreOffice 26.8.2 x64 MSI, verifies its SHA-256, and extracts it into a private build directory without installing it into the system. The download is approximately 359 MiB and provisioning requires at least 4 GiB of free disk space. For offline builds, pass `--libreoffice-msi <path>` with the matching pinned MSI, or `--libreoffice-dir <directory>` with a complete existing distribution. Use `--no-libreoffice` only to deliberately create an incomplete package. A failed extraction retains its staging directory for inspection. Bundling substantially increases package size and time; the installer checks the bundled files against the manifest.
