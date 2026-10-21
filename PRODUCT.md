# Product

<!-- impeccable:product-schema 1 -->

## Platform

native desktop (Slint / Rust)

## Users

People who record meetings on their desktop and need searchable transcripts, speaker labels, summaries, and exports. People also organize recurring work by project, review source evidence, and carry confirmed context into their next meeting.

## Product Purpose

Overtalk turns locally recorded conversations into a useful meeting notebook. The recording, transcript, and editable notes stay accessible together.

## Positioning

A local-first desktop meeting notebook with optional, explicitly configured AI providers. Reviewed local speech checkpoints, reversible transcript analysis, and source-grounded summaries give the user control over processing and corrections.

## Operating Context

The current desktop UI is native Slint with a Rust controller and a separate Rust service for SQLite, audio, speech, summaries and integrations. The Next.js/React/Tauri implementation is retained as a reference and alternative frontend, not the current desktop UI. The native build targets Linux, Windows and macOS; runtime recording and screen-capture permissions require verification on each OS. GPU workers can run under an optional launcher named by `OVERTALK_GPU_LAUNCHER`.

## Capabilities and Constraints

Recording, imported audio, transcript search, local or bring-your-own-key summaries, custom templates, PDF/DOCX/Markdown exports, speaker naming and merging, and opt-in CLI/MCP/webhook connections are implemented. Advanced speech requires separately installed Python environments and reviewed model downloads. Automatic speaker labels are provisional; remembered identities require the user to confirm whose voice is being enrolled. Quality depends on language, noise, model and hardware; published benchmarks do not prove universal superiority.

## Brand Commitments

The Overtalk name and logo variation 17b are settled. Preserve the established native visual language, readable full-window layout and semantic color. Preserve the original MIT license and dependency notices. No invented pricing, enterprise support, certification or customer claims.

## Evidence on Hand

Source code, regression tests, dated primary-source research under docs/research/, real CPU/CUDA speech smoke results, and word error rates for the default speech engine on the AMI Meeting Corpus (evaluations/ami/). The synthetic meeting benchmark's evaluation is still in progress; it is not evidence of representative real-world accuracy. There are no customer testimonials.

## Product Principles

- Keep local processing useful without a cloud account.
- Make external connections deliberate and revocable.
- Preserve recordings and allow transcript corrections to be undone.
- Make model limitations and provisional speaker labels understandable.

## Project Workspace

Projects group meetings, recurring series, people, vocabulary and reviewed context. Acoustic voice profiles are scoped to their project; uncertain matches remain unnamed. Learned context requires an exact source and explicit acceptance. Review, notes history, reminders, queued jobs, encrypted backups, multilingual reading and redacted copies are exposed in the native UI.

Optional integrations prepare reviewable ticket/document changes for Linear, Jira, Confluence and Google. Account credentials and provider permissions require user configuration; no live-account write is implied by fixture or local tests. The local meeting widget uses opt-in app/browser/calendar signals and never joins a call as a bot. Optional video uses an explicit source picker and OS permissions.
