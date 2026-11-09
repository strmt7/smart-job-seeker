# Reviewed local dependency patch

This directory contains the complete `libsqlite3-sys 0.39.1` registry archive,
including its MIT license and upstream SQLite notices. `UPSTREAM_PROVENANCE.json`
records the archive checksum and every original file hash. Git preserves these
bytes without line-ending conversion.

Only the normalized `Cargo.toml` changes:

- The package version adds `+smartjobseeker.1` to identify this modified build.
- `sqlcipher-src` changes from `=419.0.6` to `=420.0.0`.

No upstream Rust, bindings or C source was edited. The original manifest is
retained as `Cargo.toml.orig`. The source pin uses SQLCipher 4.20.0 / SQLite
3.54.1, rather than the older encrypted engine pinned by the published binding.
The plain bundled SQLite source is also 3.54.1. Both are verified at runtime in
the store tests; a package-version audit alone is insufficient.

Run `python tools/verify_sqlite_vendor.py` to verify provenance and the exact
manifest patch. Application semantic indexing excludes `third_party/` to keep
large upstream amalgamations out of application search and embedding memory.
Inspect this dependency by exact path when needed. Remove this local patch when
a reviewed upstream release supplies the required encrypted source version.
