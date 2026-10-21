# Dependency checks

CI uses locked Rust/pnpm dependencies, immutable GitHub Action commit hashes and
read-only repository permissions. Scheduled checks refresh the advisory database
weekly. `scripts/audit-dependencies.py` retains the full upstream JSON reports as
artifacts; it fails closed on malformed audit output.

The Rust gate rejects reported vulnerabilities in packages reachable through the
workspace's enabled normal/build dependency graph across all targets. Optional
packages present only in Cargo.lock remain visible in the report. Currently
`rkyv 0.7.46` and `rsa 0.9.10` fall into this category. Enabling features that make
either reachable makes the gate fail. Informational upstream maintenance and
soundness warnings are reported separately, including the GTK binding warning;
this gate does not mean every upstream warning has been eliminated.

The retained web UI has two narrowly scoped, tested advisory exceptions:

- `@tiptap/core 2.27.3`, GHSA-cp6q-959q-f8rh: this release already defines JSON
  `__proto__` as an own property rather than changing the attribute object's
  prototype. The scanner only recognizes the later 3.x release. The regression
  exercises the installed editor dependency and rejects inherited executable
  attributes.
- `braces 3.0.3`, GHSA-vfj7-8cjw-p6xm: the committed pnpm patch limits parse
  nesting before recursive traversal. Tests exercise both brace and parenthesis
  attacks through normal and expanded patterns, as well as normal glob behavior.

Both exceptions require the exact package/version/advisory tuple and successful
exploit regression tests. They do not suppress other findings. Remove them when
the compatible upstream packages incorporate recognized fixes.

Python worker dependencies are resolved and checked with pip-audit. The external
AI frameworks remain optional, separately installed runtimes; users should keep
those environments updated as well. No API credentials, private meetings or
recordings are included in test fixtures or CI artifacts.
