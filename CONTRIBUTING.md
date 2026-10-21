# Contributing to Overtalk

Pull requests target `main`. Use focused branches such as `feature/speaker-review` or `fix/audio-recovery`. To build from source, follow [docs/BUILDING.md](docs/BUILDING.md).

Use commit subjects in `type(scope): description` form, for example `fix(speech): reject corrupt model downloads`.

Follow the ownership boundaries in [the codebase map](docs/CODEBASE-MAP.md) and the deduplication and verification rules in [AGENTS.md](AGENTS.md). The native Slint and web interfaces share service behavior; preserve documented compatibility APIs.

Run the local checks in [AGENTS.md](AGENTS.md#style-and-verification) that match your change: `cargo fmt`, `cargo clippy` and the affected crate tests (`cargo test -p sonefold --lib` for the service, `cargo test -p overtalk-native` for the native UI) for Rust; `pnpm typecheck`, `pnpm test` and `pnpm build` in `frontend/` for the web interface; and the worker, companion and script unit tests for Python. Hardware-dependent tests require the actual device; report skipped checks and the reason.

The Python checks are the ones the CI workflow runs, from the repository root:

```sh
python3 -m pip install ruff==0.17.1
ruff check --no-cache --select F401,F841,F821,RUF059 --exclude '**/vendor/**' \
  speech-worker/sonefold_speech speech-worker/tools speech-worker/tests scripts integrations evaluations
python3 -m unittest discover -s scripts/tests
python3 -m pip install -r speech-worker/requirements.txt -r evaluations/meetingbench/requirements-metrics.txt
PYTHONPATH=speech-worker python3 -m unittest discover -s speech-worker/tests
python3 -m unittest discover -s integrations/tests
python3 scripts/verify-resource-packaging.py
python3 -m unittest discover -s evaluations/meetingbench -t .
python3 -m unittest discover -s evaluations/ami -t .
```

Install the Python requirements into a virtual environment, not the system Python.

The CI workflow in `.github/workflows/ci.yml` is not running yet. Once it runs, a pull request must pass it before merging. Until then, list the checks you ran and their results in the pull request.

Include the problem, resulting behavior, linked issues, validation and material limitations in each PR. Attach screenshots for UI changes. Keep generated model weights, recordings, transcripts, configuration tokens and signing keys out of commits. Preserve license and dependency notices. Complete the PR template and obtain maintainer review.

Report security vulnerabilities privately as described in [SECURITY.md](SECURITY.md), not in public issues.

## How AI tools are used here

Much of Overtalk's code, tests and documentation was written with AI coding
assistants: mainly Claude, through Claude Code, and OpenAI Codex for parts of
the evaluation tooling and the grading of benchmark results. The design concept
images were generated with image models. Commits made with Claude carry a
`Co-Authored-By: Claude` trailer. The maintainer directs and reviews this work
and decides what is kept, and every change must pass the same tests, lints and
checks as any other change. Contributions made with AI assistance are
welcome on the same terms: say so in the pull request, and make sure you
understand and can explain every line you submit.
