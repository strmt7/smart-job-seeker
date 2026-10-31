# Windows GPU memory snapshot — 2026-10-31

The local settings panel can request a read-only snapshot of dedicated GPU memory. On Windows the `gpuviewer-core` 0.1.1 WDDM backend uses OS adapter information and the system GPU Adapter Memory performance counter. Missing counters, invalid values, and truncated adapter lists are reported as incomplete or unavailable. Other operating systems report the feature as unsupported. The snapshot is not a reservation, a prediction of model memory, or a guarantee against changing load. The snapshot alone does not start a model.

One Windows host check on an AMD Radeon RX 9070 XT observed a complete snapshot and compared its used-memory reading to a separate performance-counter sample within 1 GiB. This single comparison does not establish accuracy across drivers, adapters, multiple GPUs, or rapidly changing workloads.

The dependency is available under MIT or Apache-2.0. Its [documented Windows backend](https://docs.rs/gpuviewer-core/latest/gpuviewer_core/wddm/index.html) distinguishes system adapter memory counters from the calling-process-only DXGI usage view. A robots-checked Crawl4AI 0.9.5+flitzzip.portable2 capture returned HTTP 200 and SHA-256 `11c6879a9620d97f9a8be9fa0e26c136de3f37bc8cbe6ce7e8137983909a1324`; the native capture is retained locally in `target/gpuviewer-wddm-crawl.json`.

The workspace-wide license and advisory checks did not pass: the license policy rejects two pre-existing certificate-data licenses, and the current advisory data reports findings in pre-existing dependencies. The advisory results for the prior and updated lockfiles had the same three vulnerability IDs and five warnings; this comparison does not establish that the rest of the dependency graph is safe.
