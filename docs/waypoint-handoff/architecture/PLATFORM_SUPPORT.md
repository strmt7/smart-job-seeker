# Platform support contract to qualify in G1

Primary release target: Windows x64 on Microsoft-serviced Windows 11 builds available at release. The implementation must record an exact OS/build support matrix rather than claiming all Windows versions. Windows 10 compatibility is a separate gate; verify current servicing/security arrangements before promising support. ARM64 and non-Windows targets are not initial release claims.

No admin-only development toolchain, Docker, WSL, Python or Node is required by the end-user app. Development and optional handoff-QA tools are separate. Installer technology, code-signing identity and offline/online model-delivery paths are selected after a clean-account deployment spike. Models and browser/runtime dependencies are explicitly named, licensed and versioned.

Qualify at least an AMD RX 9070 XT-class 16 GB GPU and an available 16 GB NVIDIA reference configuration before making cross-vendor claims. Record CPU, system RAM, driver, display setup and disk capacity; do not treat discrete GPU capacity as the entire machine requirement. Test memory pressure on realistic 16 GB and 32 GB system-RAM configurations and publish the resulting minimum/recommended requirements instead of inventing them now.

The native UI and inference runtime may use different graphics APIs. Measure aggregate WDDM budgets, dedicated/shared memory and behavior with a visible browser. Exact kernel/backend correctness takes precedence over a language-purity or throughput claim. A supported model is a pinned model/runtime/driver/workload combination, not a filename.

For lifecycle source context, see source-register S49/S50/S101. Recheck at release because servicing status changes.
