Capabilities are declared inline in `../tauri.conf.json`.

Keep this directory in Git even when there are no separate capability files.
`tauri-build` watches it unconditionally; a missing watched directory makes
Cargo rerun the build script and recompile the backend on every invocation.
This Markdown file grants no permissions and is ignored by the capability loader.
