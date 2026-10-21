# Repository Guidelines

## Continuity and architecture

Maintainers may keep an untracked continuity log at
`docs/PROJECT-WORKSPACE-IMPLEMENTATION.md`; when it exists, read it first for
resumed work and update its verified state and remaining gates before handoff.
Never commit credentials, recordings or transcripts.

Overtalk's desktop UI is native Slint/Rust in `native/`, backed by the Rust service
in `frontend/src-tauri/`. `frontend/src/` is a supported Next.js/React alternative,
not dead code simply because the native UI is the default. See
`docs/CODEBASE-MAP.md` for ownership and `docs/NATIVE-UI.md` for native builds.

- `native/ui/`: Slint presentation; `native/src/controller/`: UI feature handlers.
- `frontend/src-tauri/src/`: shared service behavior, audio, AI, storage and security.
- `crates/`: protocol, documents, voice memory, capture, bridge and worker isolation.
- `llama-helper/` and `speech-worker/`: isolated inference processes.
- `scripts/`: development and verification entry points; `evaluations/`: benchmarks.

## Contributing without duplication

Apply YAGNI to each change: name the current user need and actual callers before
adding a capability. Do not add unused pools, logging queues, strategy layers,
configuration knobs or extension hooks for possible future requirements. Remove
superseded paths, keep one source of truth for initialization, and use derives
when they express the exact existing behavior. Fewer lines alone is not a reason
to combine different responsibilities or remove recovery/security checks.

1. Search for the behavior and its callers with `rg` before adding a new helper,
   component, command, configuration key or dependency. Extend the owning module.
2. Keep shared behavior in the service or an existing domain crate. UI controllers
   translate user actions and service replies; do not copy storage, permission,
   provider or export rules into either UI. Use typed service commands/events.
3. Extract genuinely shared behavior after identifying real callers. Prefer a
   small domain-specific interface over a generic `utils` collection, pass-through
   wrapper, boolean-mode framework or abstraction for an imagined future caller.
   Similar syntax alone is not enough: preserve different authorization,
   cancellation, timing, platform and lifecycle requirements.
   Record the shared invariant and the intentional caller differences before
   extracting; compare lock/cleanup lifetimes and `.await` points afterwards.
4. Keep related state transitions together. Split large modules by responsibility,
   not arbitrary line count; use the narrowest visibility (`pub(super)` for
   controller siblings). Avoid new global state and circular imports.
   Coordinate concurrent lazy initialization in the state owner. Failed or
   cancelled initialization must remain retryable; an initializer may only repeat
   side effects that are safe to retry.
5. `frontend/src-tauri/src/lib.rs` registers commands;
   `scripts/generate-native-dispatch.py` generates both dispatch adapters.
   Change the source/generator and regenerate; never independently patch generated
   dispatch or expand an allowlist merely to silence a check. Run the generator's
   `--check` mode to verify committed output; it performs no writes.
6. Treat unused-code tools as evidence, not deletion authority. Check tests, CLI
   entry points, macros, `cfg`/platform branches, dynamic imports, Slint callbacks
   and public compatibility contracts. Document justified scanner exceptions next
   to their configuration. Do not mark all source files as entries to hide findings.
   Check named/default exports and dynamic imports explicitly; compiler references
   take precedence over an unused-export heuristic.
   Inspect class methods and internal Rust visibility too; export graphs miss
   unused members. Keep generated Slint internals separate from maintained code.
7. Remove superseded implementations instead of keeping `-old`, `_old`, `.backup`
   or copied versions in source control; Git retains history. Keep meaningful
   regression fixtures, migration history and explicitly documented compatibility.
   Edit vendored code only with its provenance and maintained patch policy.
8. Share tooling through importable modules (for example `scripts/service_client.py`),
   not by dynamically importing a verification CLI. Derive script paths from the
   script location and pass subprocess argument arrays rather than shell strings.
9. Add dependencies only when existing code cannot reasonably provide the behavior.
   Update manifest and lockfile together; keep unrelated versions unchanged.
   Do not duplicate a package across dependency groups without a concrete reason.
10. Put pure domain tests beside their owner. Large Rust suites may live in the
    owner's `tests.rs` child module, preserving private access. Keep caller-specific
    regressions where their distinct workflow can be exercised. Expected results
    must be independent of the shared helper being tested.
    Persistence helpers resolve writes after transaction commit, preserve atomic
    rollback and distinguish best-effort checkpoints from explicit user actions.
    Test aborts after successful requests; browser adapters do not prove browser
    durability, so verify changed transaction timing in a real browser too.
11. Wire user actions through the existing domain flow. Reserve a successful empty
    selection for cancellation; unsupported or failed actions need an explicit
    error. Persist preferences through their owner and refresh the UI from the
    saved result, rather than maintaining a separate interpretation of settings.
12. Give background work a real consumer and a bounded lifetime. Metrics need a
    reader and a retention policy; reuse existing counters instead of parallel
    queues. Async UI replies must still match the active request before publication.

## Style and verification

Use TypeScript strict types, PascalCase React components, camelCase functions,
`use*` hook names and `@/` imports. Follow nearby indentation (usually two spaces
in TypeScript, four in Rust) and rustfmt. Avoid unrelated mass formatting.

- Rust: `cargo fmt --all -- --check`; `cargo test -p sonefold --lib` for the service;
  `cargo test -p overtalk-native` plus affected workspace crates for native changes.
  Run `cargo clippy --release --workspace --all-targets -- -D warnings`. Fix
  warnings at their source; a fixed public wire contract may use a narrowly scoped
  `expect` with a reason. Keep vendored compatibility patches documented.
- Frontend (from `frontend/`): `pnpm install --frozen-lockfile`, `pnpm typecheck`,
  `pnpm test`, `pnpm build`. `pnpm lint` is a compatibility alias for type checking,
  not ESLint. Tests run in isolated Bun processes to avoid module-mock leakage.
  Keep `noUnusedLocals` and `noUnusedParameters` enabled. Remove obsolete React
  state, effects and handlers together; setter-only state still causes renders.
  Keep side-effect import checking and Knip's runtime-cycle gate enabled. CSS
  declarations describe the loader contract; the production build verifies assets.
- Scripts: `python3 -m unittest discover -s scripts/tests`. Verify changed service
  clients through the real disposable-profile workflows in `scripts/verify-*.py`.
- Quality: `python3 scripts/audit-code-quality.py --output /tmp/overtalk-quality`;
  provision tools using `docs/QUALITY-TOOLS.md`. Knip's
  checked-in entries include tests and operational scripts. Check exports/types
  as well as whole files. Review clone reports;
  do not enforce zero duplication or infer Slint coverage from jscpd.
- Add behavior-level regression tests for fixes. Pure moves should preserve method
  bodies and pass existing tests. Exercise recording/transcription for audio
  lifecycle changes. A Linux check does not prove Windows/macOS compatibility.

## Commits and handoff

Follow `CONTRIBUTING.md`'s `type(scope): subject` style and the user's requested
branch/merge target. Keep unrelated work out of commits. PRs use
`.github/pull_request_template.md`, describe concrete behavior, relevant validation
and remaining limits; include screenshots for visual changes. Report actual test
results and CI blockers, never anticipated results. Require passing CI and a
maintainer review before merging. Keep audit artifacts outside
source control unless they are small, sanitized and useful to future contributors.
