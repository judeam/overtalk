# Updated bundled SQLite

The libsqlite3-sys 0.30.1 Rust binding ABI is retained for SQLx 0.8 compatibility.
The bundled public-domain sqlite3.c, sqlite3.h and sqlite3ext.h are SQLite 3.54.1.
Exposed version constants in the pregenerated bindings were updated accordingly.

Official source: https://sqlite.org/2026/sqlite-amalgamation-3540100.zip
Verified SHA3-256: 92281c86e2957f9b9305890f9ac04204caa7b677cc868dc326d942689df28e30
Published digest: https://sqlite.org/download.html

This updates the C engine (which a Rust-only advisory scan does not inspect),
including the FTS5 secure-delete integrity-check fix first shipped in 3.46.1.
The upstream crate's bundled SQLCipher sources (`sqlcipher/`) are removed: Overtalk
never enables the `bundled-sqlcipher` features, which would fail to build here.
