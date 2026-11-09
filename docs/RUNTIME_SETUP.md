# Windows runtime setup

Updated 2026-11-09. These mechanisms are implemented in the prototype; a clean
Windows virtual-machine installation is still an acceptance gate.

## Default installation

The default Windows x64 package includes its own Python and PDF libraries,
headless Chromium, LibreOffice, Ollama, OpenSSL and required Microsoft C++ runtime
DLLs. Packaging automatically downloads missing pinned distributions, verifies
them and bundles their notices. It does not depend on finding these applications
on the build host. An explicit `--libreoffice-dir` or `--chromium-dir` can reuse
a distribution chosen by the packager; implicit host discovery does not choose
the default converter or application browser.

`install.ps1` verifies the package inventories before copying and verifies the
installed dependency files again. The default destination is the current user's
Programs folder. It creates no machine-wide package installation, service, PATH
entry or replacement of another application's files. An existing destination
is preserved; choose a new directory for a side-by-side installation. The optional
Start Menu shortcut is also never overwritten.

The package builder extracts LibreOffice using Windows Installer's administrative
extraction mode. Microsoft C++ DLLs are extracted from standard cabinets in the
hash- and publisher-verified Microsoft redistributable, using Windows' cabinet
utility and read-only MSI file tables. The redistributable installer is not run.
Its DLLs sit beside Smart Jobseeker, LibreOffice and Ollama. The original
Microsoft runtime license document is included. No system runtime is replaced.
Redistribution remains subject to the included component licenses and Microsoft's
[runtime terms](https://visualstudio.microsoft.com/license-terms/vs2026-ga-visualcpp-v14-redist-runtime/).

Downloads and archive copies are streamed; large Ollama archives are not loaded
into RAM. Archive extraction has size, entry-count and Windows-path checks.
Provisioning failures remain errors with diagnostic staging where appropriate;
they never trigger a global repair or deletion of an existing installation.
Network access, enough disk space and OS permission to execute the components
are required. Offline packaging accepts the exact verified local archives.

## PDF setup and repair

In Settings, leave **Custom Python executable** empty and choose **Set up PDF
tools**. A background native installer downloads Python and its PDF wheels into
a fresh per-user staging directory. No existing Python, pip, venv or package
manager is needed. Every archive must match its exact size and SHA-256, and
paths, duplicates, unsupported entries and expansion budgets are checked before
activation. Downloads are bounded to one small artifact at a time.

A hidden, memory- and time-bounded import check must pass before the complete
runtime is moved into place and the active-runtime pointer is replaced. A failed
download, extraction or check preserves the previous active runtime and cleans
the new private staging directory. The existing asynchronous status/recovery
controls remain available. A deliberately supplied Python path instead creates
a separate virtual environment; it never installs into that Python's packages.

## Browser and model runtime

An explicit browser override takes precedence, followed by bundled headless
Chromium and optional installed browsers. The bundled engine runs application
automation in the background. It does not replace the user's default web browser.
The local web workspace still opens in the user's chosen browser.

Pinned browser builds do not update themselves. The bundled testing engine is
currently qualified for fictional local fixtures, not unrestricted hostile web
content. Use a supported, up-to-date browser override for public-site inspection;
development must not interact with real application forms.
Browser update/serviceability and clean-host compatibility remain open gates.

Ollama is bundled by default; Settings can start the private runtime. Model
weights are separate and selected explicitly. Existing RAM/GPU admission and
concurrency limits still apply. The application never unloads another workload's
model to make space. Full automatic repair of a deleted Ollama or LibreOffice
distribution is not yet implemented in Settings; reinstalling a complete bundle
provides those components without a separate system installer.

`--no-ollama`, `--no-libreoffice` and `--no-chromium` deliberately produce minimal
development bundles with missing capabilities. They are not the default
installation. Development additionally requires a Rust/MSVC compiler toolchain;
the installed application does not. The build helper provisions its own verified
OpenSSL SDK when needed, without installing OpenSSL system-wide.
