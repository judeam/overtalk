# Updated bundled SQLite

The libsqlite3-sys 0.37.0 Rust binding ABI is retained: it is the newest release
that both SQLx 0.9.0 (`>=0.30.1, <0.38.0`) and rusqlite 0.39.0 (`0.37.0`) accept,
and Cargo allows only one package that links `sqlite3`. Upstream source: the
published crate, registry checksum
`b1f111c8c41e7c61a49cd34e44c7619462967221a6443b0ec299e0ac30cfb9b1`, built from
rusqlite commit `2a1790a69107cd03dae85d501dcbdb11c5b32ef3` (`.cargo_vcs_info.json`).
It bundles SQLite 3.51.3.

The bundled public-domain sqlite3.c, sqlite3.h and sqlite3ext.h are SQLite 3.54.1.
The version constants in the pregenerated bindings (`SQLITE_VERSION`,
`SQLITE_VERSION_NUMBER`, `SQLITE_SOURCE_ID` and the `SQLITE_SCM_*` values in
`sqlite3/bindgen_bundled_version*.rs`) were updated to match its header.

Official source: https://sqlite.org/2026/sqlite-amalgamation-3540100.zip
Verified SHA3-256: 92281c86e2957f9b9305890f9ac04204caa7b677cc868dc326d942689df28e30
Published digest: https://sqlite.org/download.html

This updates the C engine (which a Rust-only advisory scan does not inspect).
The upstream crate's bundled SQLCipher sources (`sqlcipher/`) are removed: Overtalk
never enables the `bundled-sqlcipher` features, which would fail to build here. Its
`Cargo.lock` is not vendored (the crate's `.gitignore` excludes it, and Cargo ignores a
path dependency's lockfile). Remove this fork when an upstream libsqlite3-sys release
that SQLx accepts bundles SQLite 3.54.1 or later.
