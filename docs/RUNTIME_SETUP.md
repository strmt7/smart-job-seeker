# Windows runtime setup

Updated 2026-11-10. These mechanisms are implemented in the prototype; a clean
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

Settings > **Local tools and setup > Set up tools** now installs all missing
core tools in the background. The selector can instead repair the private
browser, document converter or Ollama distribution. PDF tools are bootstrapped
first when needed, without using or changing a host Python installation.

Missing default tools can also be provisioned when starting Ollama, opening a
browser inspection, saving a [source-page copy](SOURCE_COPY_WORKFLOW.md) or converting a document. PDF operations can bootstrap their
missing default worker; **Check PDF tools** remains a read-only probe. Explicit
custom paths retain precedence and their diagnostics. Installing a private copy
does not replace an override or automatically repeat a form interaction.

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
model to make space. Missing-runtime setup and the Settings repair action reuse
the same pinned distributions as packaging. Repair creates a new private version,
verifies its complete file inventory and version command, then atomically changes
its per-user pointer. Prior versions and unrelated installations are preserved.
The browser status display follows the selected private runtime. Models are not
downloaded or loaded by this tool-setup action.

Each setup worker is hidden and limited to 768 MiB of process-tree commit and
20 minutes. Setup requires a reliable observation of at least three GiB of free
host RAM and checks again before each component. This is a snapshot, not a
reservation; unrelated workloads and Windows Installer service allocations are
outside that worker limit. Disk admission requires two GiB for browser setup,
four for the converter or twelve for Ollama. Downloads stream to disk, and
archives retain their pinned size/hash, path, count and expansion checks.
Ollama's two compressed runtime archives total about 1.8 GB; weights are separate.

There is one automatic runtime installer at a time. Page reload can recover the
in-process task; terminal status can be polled repeatedly. Failed stages remain
with diagnostics because a Windows Installer administrative-extraction service
can outlive its client after timeout. No failed result activates the new runtime
or deletes its predecessor. A later setup uses a separate directory. An app
restart does not resume a partial download automatically. No machine-wide package
installation, service registration, global PATH change or replacement of another
application is requested. The previously described administrative MSI extraction
and read-only cabinet extraction are reused, rather than running the normal
LibreOffice or Microsoft runtime installers.

## Recovery qualification, 2026-11-09

The explicit ignored native test downloaded pinned private Python/PDF libraries
and the browser, activated a complete 288-file browser distribution, opened an
owned fictional file page through the real CDP driver and captured it. It passed
inside a four-GiB outer Windows job, in 17.432 seconds including the test command.
The browser reported `Google Chrome for Testing 157.0.8103.38`.

Private evidence: `target/runtime-repair-9Rb8dc/qualification.private.json`,
SHA-256 `7bb2682945907e431514b52236b5cbf632680f14162e1c51d66969e85525cca3`.
Its source digests bind the installer version used; a later status-only getter
was added afterward. The subsequent rendered qualification binds the updated
executable. Both are scoped observations rather than qualification on a clean VM.

The actual Settings page then downloaded and activated PDF/browser tools through
production endpoints, without substituted responses. One setup request passed
five widths (320, 390, 768, 1440 and 1920 pixels) and six actual browser zoom
factors (80, 100, 125, 150, 200 and 400 percent). The selected browser status
matched the installed path, and no UI script errors or external UI requests were
observed. The outer four-GiB job run took 19.048 seconds; worker allocations and
host/GPU peaks were not measured. Screenshots were inspected. This also exposed
an older PDF recovery GET/POST mismatch, corrected with a transport assertion.

Final private rendered evidence:
`target/runtime-setup-ui-lBQwBZ/result.private.json`, SHA-256
`f916f8f7603c6cac3684fac75616dc15906e5c7895ebd1f4b8486ad39af6248a`.
The development script is `apps/web_app/scripts/qualify-runtime-setup.cjs` and
requires an explicitly chosen `--browser` executable plus the existing private
Playwright development dependency. The measured invocation used the repository's
`qualify_rehearsal.run` Windows process-tree containment. All generated workspaces,
profiles, receipts and downloaded runtimes stayed under ignored `target/`.

Seven offline Python tests use fictional files with the actual packager's sink,
archive, runtime-DLL and license paths. They cover corruption, missing/extra
members, traversal/Windows aliases, duplicate paths, disk refusal, failed probes
and refusal to overwrite an existing output directory. Native tests cover
pointer traversal, incomplete versions, prior-pointer preservation, missing
files, RAM boundaries and retryable status. Ordinary Rust tests cannot implicitly
download a missing dependency. The existing package checks also passed.

Subsequent [live converter/Ollama qualification](RUNTIME_REPAIR_QUALIFICATION.md)
passed actual private downloads, DOCX/PDF conversion and a model-free private
server. It also fixed a first-use worker-lease conflict and isolated version-probe
profiles. Clean-Windows installation, installer-service failure schedules,
restart cleanup, updater servicing and full application first-use recovery
remain open. Rust/MSVC development-toolchain installation remains separate.

`--no-ollama`, `--no-libreoffice` and `--no-chromium` deliberately produce minimal
development bundles with missing capabilities. They are not the default
installation. Development additionally requires a Rust/MSVC compiler toolchain;
the installed application does not. The build helper provisions its own verified
OpenSSL SDK when needed, without installing OpenSSL system-wide.
