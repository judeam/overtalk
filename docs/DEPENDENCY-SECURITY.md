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

The retained web UI currently has no advisory exceptions. Tailwind CSS 4 and
BlockNote 0.56 (TipTap 3) removed the earlier `braces 3.0.3` and
`@tiptap/core 2.27.3` exceptions, together with the pnpm patch for braces.
`.github/verify-dependency-security.cjs` still exercises the installed TipTap
`mergeAttributes` against GHSA-cp6q-959q-f8rh.

An exception requires the exact package/version/advisory tuple in
`scripts/audit-dependencies.py` and a successful exploit regression test against
the installed dependency. It does not suppress other findings. Remove it when the
compatible upstream package incorporates a recognized fix.

Python worker dependencies are resolved and checked with pip-audit. The external
AI frameworks remain optional, separately installed runtimes; users should keep
those environments updated as well. No API credentials, private meetings or
recordings are included in test fixtures or CI artifacts.
