# Waypoint local web prototype

On Windows, build the prototype from the repository root with:

```powershell
./apps/web_app/scripts/build-windows.ps1
```

The helper requires Rust, the MSVC build tools and a compatible x64 OpenSSL developer installation (headers, import library and runtime DLL). It checks the standard `Program Files/OpenSSL-Win64` location; pass `-OpenSslRoot` to use another installation. It builds one job at a time by default and copies the OpenSSL runtime next to `target/debug/waypoint_web.exe`. Add `-Release` for a release build. This is a build helper, not an installer or a dependency downloader.

Start the resulting executable to open the local browser workspace. The Windows default build uses SQLCipher for the workspace database with a key protected by the current Windows account's DPAPI. When an existing plaintext workspace is migrated, the original database is deliberately retained and the interface reports that it remains on disk; protect it as private data. Application-record folders remain readable as requested. The prototype does not send applications from its interface.

The executable and its OpenSSL DLL must be distributed together. Installation, portable dependency provisioning, backup and key recovery are not yet complete. See `docs/WEB_PROTOTYPE_CHECKPOINT.md` for current capabilities and limitations.

The project uses free local tools and must not require paid APIs. Wider-web search uses a user-configured SearXNG-compatible provider; use a free or self-hosted instance. The app cannot determine an independent provider's pricing or guarantee its availability. Local-model features use Ollama rather than a paid inference API.

To create a local prototype ZIP after a build, use Python 3.11 or newer:

```powershell
./apps/web_app/scripts/build-windows.ps1 -Release
python ./apps/web_app/scripts/package-windows.py --release --output ./prototype-local/waypoint-release.zip
```

The output path must be new and inside the repository. The packaging step checks the build receipt and collects local license files for the Windows runtime dependency graph. Extract the ZIP and run `install.ps1` to copy it into a new per-user folder and create a Start Menu shortcut, or run the executable in the extracted `app` folder. Existing installation folders and shortcuts are not overwritten. The package is an unsigned prototype; Microsoft Visual C++ runtime, a Chromium browser, Ollama, and optional document tools are not installed by it. The dependency license inventory still needs review before redistribution.
