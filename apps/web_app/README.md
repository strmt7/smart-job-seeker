# Smart Jobseeker local web prototype

Document-page and captured-form images open in a shared full-size viewer. Use
mouse-wheel zoom, drag to pan, or the corner grip to resize it. The original
image and application-review state remain unchanged.

On Windows, build the prototype from the repository root with:

```powershell
./apps/web_app/scripts/build-windows.ps1
```

The helper requires Rust and the MSVC build tools. Unless `-OpenSslRoot` or `OPENSSL_DIR` explicitly selects an SDK, it downloads the pinned, signed OpenSSL 4.1.0 x64 MSI (installer revision 2), verifies it and extracts the development files into `target/waypoint-openssl-4.1.0-2`. It does not install OpenSSL system-wide or silently choose a host installation. Provisioning needs network access and at least 1 GiB of free disk space; the download is about 58 MiB. For offline setup, pass `-OpenSslInstaller <path>` with that exact MSI. A verified cache is reused; incomplete or changed caches are rejected. The helper builds one job at a time by default and places its owned runtime beside the executable. Add `-Release` for a release build. Rust and MSVC are not installed by this helper. The repository pins Rust 1.99.1 without changing the global default toolchain.

Release builds request Rust's static C runtime by default; debug builds retain Cargo's caller-supplied CRT settings. Use `-StaticCrt` to request static CRT explicitly, or `-StaticCrt:$false` to retain the caller's settings for a release build. The build receipt and package manifest record whether the helper requested static CRT; they do not verify the resulting imports. Bundled components may still have their own runtime requirements, and clean-host installation remains unverified. See `docs/g1/WINDOWS_STATIC_RELEASE_2026-11-01.md` in the source repository for the release-build evaluation.

The helper places the OpenSSL runtime and receipt next to the executable reported by Cargo, including with a custom target directory. When a later build uses a different OpenSSL DLL with the same name, the helper replaces it only if the previous Smart Jobseeker build receipt identifies the existing file. It keeps the old DLL in `waypoint-openssl-backups` beside the executable. An unrecognized DLL is left untouched.

Use `./apps/web_app/scripts/build-windows.ps1 -Test` to run the web-app tests or `./apps/web_app/scripts/build-windows.ps1 -Clippy` to lint its targets with warnings treated as errors. Both commands configure the same detected OpenSSL paths as the build.

To run a specific ignored integration test, use `-Test -Ignored -TestFilter <test-name>`. Inspect its prerequisites first: ignored tests can start local browsers, access the network or require installed tools.

Settings > Local tools and setup > **Set up tools** automatically installs missing PDF, browser, document-converter and Ollama runtimes in private folders. Its selector can repair an individual private runtime. Missing default tools can also be installed when first needed; explicit custom paths are preserved. Downloads, extraction and hidden checks are bounded, and failed setup leaves previous runtimes active. Model weights are selected separately. The dedicated PDF setup still accepts an optional custom Python and creates a separate environment. No host Python or pip is required for the automatic path. See [runtime setup and measured limits](../../docs/RUNTIME_SETUP.md).

For development tests, use `./apps/web_app/scripts/build-windows.ps1 -Test`; add `-TestFilter <name>` to run one test. The helper configures the same OpenSSL environment for tests as for builds.

Start the resulting executable to open the local browser workspace. On Windows, use the notification-area menu to reopen the workspace or exit; double-click the icon to reopen it. The Windows default build uses SQLCipher for the workspace database with a key protected by the current Windows account's DPAPI. When an existing plaintext workspace is migrated, the original database is deliberately retained and the interface reports that it remains on disk; protect it as private data. Application-record folders remain readable as requested. Smart Jobseeker has no application-submit feature, but a website can transmit or submit data in response to form interactions. This prototype cannot yet prevent all such behavior; do not interact with real application forms when an application must not be sent.

For application inspection, an explicit `WAYPOINT_BROWSER` executable takes priority, followed by bundled headless Chromium and optional installed browsers. The default package automatically provisions a pinned browser with its original notices. It does not replace the default browser used to view the local workspace.

Use an up-to-date supported browser override for public-site inspection. Pinned testing browsers do not update themselves; the bundled engine is currently qualified for fictional local fixtures. Browser serviceability, hostile-content isolation and clean-host compatibility remain open acceptance gates. See [runtime setup](../../docs/RUNTIME_SETUP.md).

Public-board and batch web searches continue while the local app is running even if the web page is closed. Board results are saved incrementally. The latest web-search batch is sealed to the Windows user for recovery after restart; an active batch is normally checkpointed every five seconds and saved once more at normal shutdown. An interrupted batch is shown as incomplete and is not automatically resumed. A crash can lose pages since the last checkpoint, and the interface reports checkpoint or completed-history saving failures.

After upgrading an existing workspace, duplicate checking revisits older requisitions in the background. It checks at most one eligible job or skips a page of 64 jobs per scheduled step, and resumes after interruption. Existing duplicate decisions are retained. The interface reports the recheck while it is running; duplicate search waits for indexing to finish. The comparison remains bounded and can miss matches.

Up to 100 selected search links are sealed and restored from the workspace. Selecting a link does not inspect or apply for it. If another window changes the selection, reload before saving further changes.

The executable and its OpenSSL DLL must be distributed together. Installation, portable dependency provisioning, backup and key recovery are not yet complete. See `docs/WEB_PROTOTYPE_CHECKPOINT.md` for current capabilities and limitations.

The project uses free local tools and must not require paid APIs. Wider-web search uses a user-configured SearXNG-compatible provider; use a free or self-hosted instance. The app cannot determine an independent provider's pricing or guarantee its availability. Local-model features use Ollama rather than a paid inference API.

To create a local prototype ZIP after a build, use Python 3.11 or newer:

```powershell
./apps/web_app/scripts/build-windows.ps1 -Release
python ./apps/web_app/scripts/package-windows.py --release --output ./prototype-local/waypoint-release.zip
```

The output path must be new and inside the repository. Packaging verifies the build receipt and locked normal/build dependency licenses. The default bundle provisions verified Python 3.15.2, pypdf 7.0.0, pypdfium2 5.15.0, Pillow 13.0.0, headless Chromium, LibreOffice 26.8.2 and Ollama 0.40.15. Required Microsoft C++ DLLs are extracted from the publisher-signed redistributable into the app package, without running its installer or changing the system runtime. LibreOffice is provisioned from the pinned MSI unless an explicit directory is supplied. Model-runtime downloads total about 1.9 GB; packaging requires at least 8 GiB of free space when bundling Ollama. Offline inputs include `--python-package`, `--wheel-dir`, `--chromium-archive`, `--vcrt-installer`, `--libreoffice-msi` and `--ollama-archive-dir`. Exact size/hash checks still apply. Explicit `--no-*` flags produce minimal development bundles. See [runtime setup](../../docs/RUNTIME_SETUP.md) for installation, repair, optional overrides and remaining acceptance limits. The package remains an unsigned prototype; its manifest detects missing or damaged files but does not prove publisher authenticity. Do not distribute it as a qualified release.

Before copying, the installer checks that the destination reports enough available space for the extracted app files plus a 512 MiB reserve; free space can still change during installation. The installer includes hidden package files and verifies the executable, OpenSSL and inventoried dependencies again after copying. This second verification increases installation time, especially with bundled LibreOffice. If installation fails after copying starts, the newly created directory is retained for inspection and no success is reported.

If Cargo used a custom target directory or target triple, pass `--build-dir <directory>` pointing to the profile directory printed by the build helper. Use `--release` when packaging a release build; the receipt must match the selected profile and files.

Packaging uses an installed LibreOffice distribution if available. Otherwise, on Windows, it downloads the pinned official LibreOffice 26.8.2 x64 MSI, verifies its SHA-256, and extracts it into a private build directory without installing it into the system. The download is approximately 359 MiB and provisioning requires at least 4 GiB of free disk space. For offline builds, pass `--libreoffice-msi <path>` with the matching pinned MSI, or `--libreoffice-dir <directory>` with a complete existing distribution. Use `--no-libreoffice` only to deliberately create an incomplete package. A failed extraction retains its staging directory for inspection. Bundling substantially increases package size and time; the installer checks the bundled files against the manifest.
