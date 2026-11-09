# Dependency refresh — 2026-11-09

This is a prototype development checkpoint, not release qualification. Registry,
release and Dependabot data were queried directly and retained with native
responses outside Git. No host-wide Python, Node, OpenSSL, browser or Ollama
installation was upgraded. Repository changes remain on `main`.

## Updated graph and distributions

| Component | Selected version / behavior |
|---|---|
| Rust | 1.99.1, repository-scoped toolchain file |
| Rust graph | Latest compatible locked releases, including reqwest 0.13.5, base64 0.23.1, sha2 0.11.1, getrandom 0.4.3, quick-xml 0.42.0, zip 9.5.0 and gpuviewer-core 0.2.2 |
| PDF runtime | CPython 3.15.2 x64; pypdf 7.0.0, pypdfium2 5.15.0, Pillow 13.0.0 |
| Browser | Official current stable headless shell 157.0.8103.38; automatic default bundling |
| Converter | Pinned LibreOffice 26.8.2; default provisioning no longer reuses an implicit host installation |
| Model runtime | Ollama 0.40.15 artifact pins; running local runtime also reported 0.40.15 |
| Microsoft runtime | 14.51.36247.0; 12 individually pinned DLLs, extracted without executing the redistributable |
| OpenSSL SDK | Official 4.1.0 installer revision 2; signature, archive hash and 149-file development-tree checks |
| Development Node | Portable current LTS 26.13.0; CI uses Node 26 |
| Code index | CocoIndex Code 0.2.43 in a version-specific environment; other repositories' environments preserved |
| Web research | Reviewed Crawl4AI 0.9.5 + portable repair 2; refreshed compatible hash lock, official NLTK 3.10.4 retained |
| CI actions | Current verified release commits for checkout, Python/Node setup, artifact upload, Rust cache and Codecov; Rust action also source-pinned |

Direct Rust requirements and the resolved graph were checked separately; broad
requirements such as `serde = "1"` are not evidence of an old resolved release.
The Python research-provider lock was upgraded within its Python 3.13,
platform and declared-package compatibility constraints; unrestricted new
majors were not forced into that qualified environment. Crawl4AI's
portable source repair was not replaced by the unmodified index package.
The latest cargo-audit, cargo-deny, cargo-llvm-cov, Gitleaks and Playwright Core
versions checked were already the selected versions.

The GitHub API snapshot contained **zero open Dependabot alerts** and six fixed
alerts. This describes the remote repository at query time, not a security
certification of unpublished changes. Final local `cargo audit --deny warnings`
reported zero vulnerabilities and no warning categories; license/ban/source
policy checks passed. The embedded locked license inventory was regenerated.
The newly published GPU telemetry crate omitted license files; the retained MIT
notice was verified byte-for-byte at its recorded upstream source revision.

## Compatibility fixes and evidence

- quick-xml's string-based API and sha2's output representation were adapted in
  production code. Existing document/revision/archive-integrity tests stayed
  enabled. reqwest's retired feature was replaced with its supported Rustls
  platform-verifier feature; TLS verification was retained.
- pypdf 7 moved decompression limits to its configuration API. All workers now
  set the same bounds through that API, with compatibility for an explicitly
  chosen older runtime. Its new StandardEncoding fallback is not treated as
  proof that an unmapped replacement glyph exists. Four fictional PDF regression
  scripts passed after the fixes; their initial failures are retained.
- The web Rust suite passed **719 tests**, with 394 explicitly ignored. The rest
  of the workspace passed **373 tests**, with 163 ignored. Clippy passed with
  warnings denied. These denominators exclude opt-in live qualification.
- **159 UI scripts** passed on portable Node 26.13.0; the packaging fixture script
  also passed. The compiled UI passed **296 resize/zoom cases** with zero failures
  or script errors. A separate career-history run covered the new advice panel
  at five widths and explicit answer selection with fictional intercepted API
  responses. Its screenshots were inspected at desktop and narrow widths.
- Native PDF setup downloaded and activated a runtime and rendered a fictional
  PDF with host Python absent from the test PATH. The actual setup worker ran
  in an isolated profile; the owned process tree had a 4 GiB job limit and a
  300-second deadline. It completed in 8.124 seconds on this host. This is not
  a clean-Windows VM result or a measured memory peak.
- A component ZIP installed through Windows PowerShell 5.1 into a new private
  directory. Its Python imports passed, and the bundled browser passed a native
  field fill/readback fixture. PowerShell module-path inheritance and child exit
  code differences exposed by these runs were fixed. The OpenSSL helper also
  completed fresh SDK extraction through Windows PowerShell 5.1.
- Default converter provisioning downloaded and extracted the pinned LibreOffice
  MSI without using the installed converter. Its 19,429 files were included with
  the new app-local C++ DLLs. The first full install exceeded a 120-second test
  harness deadline. A native hashing helper retained every SHA-256 check; the
  current installer then completed source- and destination-file verification in
  119.43 seconds under a 360-second harness deadline. This was a source-hash-bound
  installer run against unchanged extracted package data, not qualification of a
  release artifact or a comparative performance benchmark.
- The installed converter bundle's app found all three private tools with only
  Windows System32 on its PATH. A fictional Unicode document converted to PDF,
  retained the expected extracted text and rendered for review. The owned test
  process tree completed in 12.148 seconds under a 4 GiB job limit. Existing
  machine-wide C++ DLLs were not removed to simulate a clean host.
- The refreshed isolated research provider completed 31 successful domains in
  32 attempts, retaining one robots-denied failure. Two oversized registry JSON
  responses were retained as failures and the versions verified through official
  native release feeds. A license short-link was robots-denied; the official
  directory led to the canonical original license DOCX, retained with its hash.

Local native captures, full logs, model traces, package ZIPs, screenshots and
test-profile paths remain under ignored `target/` or external owned tool caches.
No candidate material or credentials belong in this report or the repository.

## Remaining acceptance

The newly pinned Ollama archives have release metadata verification but have not
been rebuilt and qualified as a complete new GPU bundle. A clean Windows VM,
browser update/serviceability strategy, deleted-runtime repair for LibreOffice
and Ollama, offline failure/retry and cross-machine resource qualification remain
required. Model quality remains a separate gate: a six-field history diagnostic
passed with the local 12B model and failed completeness with the 14B model. No
real application was interacted with or sent, and no release was created.
