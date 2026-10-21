# Controlled multilingual meeting benchmark

The original 200 saved results can be reviewed in a dedicated app project:
`python3 -m evaluations.meetingbench.install_project` validates a dry run;
adding `--apply` backs up the real library and imports audio, actual ASR results,
speaker turns and the Qwen3.5-4B notes generated from those ASR results. It uses
the installed native service, so install a build with `speakerTurns` import
support first. Repeated imports reuse the same meetings and preserve later note
edits. Reference transcripts and cast names do not replace predicted results.

For the native Privatemode comparison, setup, free-plan gate, and capped runner,
see [Privatemode in Overtalk](../../docs/PRIVATEMODE.md). The comparison preserves
the original 200-case outputs and uses direct assistant review, not a local
model grading itself.
The 16 October paired comparison covered six selected cases with 12 source
controls per model; it is not a new full-corpus accuracy result.

Work in progress. All 200 structured script drafts are assembled from
assistant-authored multilingual dialogue and localized topics. They share
templates and are not 200 independent natural conversations. All 200 recordings
have now been generated. The original model run is preserved at 181 completed
meetings; the faster candidate and stronger scoring are being validated separately.
The [meeting-accuracy research note](../../docs/research/2026-10-15-meeting-accuracy.md)
covers the methods considered for improving these results.

## Scope

- 200 meetings: 100 normal, 60 hard, 40 really hard.
- Durations span 2–90 minutes. Equal counts in the declared 2–4, 4–9, 9–20,
  and 20–90 minute bands, stratified within each band. These are not uniform
  minute-by-minute samples of the whole interval.
- 2, 3, 4, 6 and 8 participants, balanced jointly with duration band within
  each difficulty. All twenty scenario families occur in each group.
- At least one monolingual example of all 30 Qwen3-ASR languages in every
  difficulty; 22 additional Chinese dialect/accent varieties across
  the corpus; 32 multilingual meetings, including English/Hindi switching.
- Language weights for additional meetings are explicit test-design assumptions,
  not empirical frequencies or population statistics.
- The current run uses **Qwen3-ASR 1.7B** and **Qwen3.5 4B**. Qwen3.8 27B and
  other model alternatives are deferred. The 4B model is evaluated on both the
  reference and ASR transcripts.
- **Scripts and reference answers were written by an AI assistant (OpenAI
  Codex), not by human annotators.** Gemini is used only for speech/voice
  synthesis. No paid script-generation model is used.
- Paid synthesis runs against a fixed budget enforced by a local cost ledger.

`python3 evaluations/meetingbench/plan.py` creates the plan, about 50.8
speaker-hours of audio, and records its estimated synthesis cost and retry
reserve. Estimates exclude taxes and promotional credits. Record actual usage.
The local ledger is independent of Google billing budgets.

## Fast execution target

The target is a complete run in under one hour. This is a target to measure,
not a validated end-to-end completion time. Script authoring, native-language review,
audio synthesis, inference, and scoring must not be conflated.

The selected model pair is Qwen3-ASR 1.7B plus Qwen3.5 4B. Keep the full 200
meeting allocation and actual speech; do not pad short clips with silence or
reuse one conversation as purportedly independent meetings to meet the deadline.

The execution strategy is concurrent cloud synthesis, local audio mixing as
clips arrive, batched ASR using the same 30-second windowing as the app, and
batched 4B summaries with the production prompts and source references. Keep
model weights loaded. Pipeline completed meetings so local evaluation starts
before the last cloud response. Do not run the 27B model in the current run.

`probe_tts_throughput.py`, `probe_asr_throughput.py`, and
`probe_summary_throughput.py` save real throughput measurements under `.runtime`.
The probe data is excluded from the evaluation set. Repeated ASR inputs are
explicitly labelled; pilot throughput is not a multilingual accuracy result.
Run the ASR probe under your own GPU-sharing launcher if you use one. The 4B probe measures the inference engine, not every stage of the
production summary pipeline.

### Staged local runner

`fast_evaluation.py` keeps diarization and recognition models warm, batches
recognition/alignment, then replays the real ASR adapter's timing and retry
logic. It never passes reference text or speaker names into speech inference.
The native Rust test runner performs both notes controls. Compatible requests
from separate meetings can share one local helper through `batched_notes.py`;
the original prompts, token limits, sampling and source checks are retained.
The shared helper allocates a larger context for independent sequences.

Install `requirements-metrics.txt` in `.runtime/metrics-venv`. The optional
`requirements-throughput.txt` belongs in a separate `.runtime/fast-venv`; it
must not replace the application's speech environment. The Transformers path
uses the existing Qwen environment. Run GPU work as below, prefixing the
interpreter with your own GPU-sharing launcher if you use one:

```sh
OMP_NUM_THREADS=4 MKL_NUM_THREADS=4 OPENBLAS_NUM_THREADS=4 \
  evaluations/meetingbench/.runtime/fast-venv/bin/python \
  -m evaluations.meetingbench.fast_evaluation \
  --output evaluations/meetingbench/.runtime/experiments/NEW-RUN \
  --native-runner target/release/deps/ACTUAL-APP-LIB-TEST-BINARY \
  --helper target/release/llama-helper \
  --backend vllm --batch-size 64 --notes-workers 8
```

Use the actual `app_lib-*` Rust **test** executable, not a GUI executable.
Omitting `--cases` runs all 200. The native test needs the ONNX Runtime library
path set in its environment. The shared socket scheduler currently targets a
Linux evaluation host. Desktop application behavior is unchanged.

Each output contains source snapshots, input/model/helper hashes, configuration,
stage timings, original model outputs, per-case scores, `statistics.json`, paired
`comparison.json` and `report.md`. Use `--resume` only with an unchanged
configuration; resumed work never establishes a fresh-run speed claim.
The explicit helper selects the freshly built engine; omitting it preserves
the frozen installation's helper. Both executable hashes are recorded.
The runner also fingerprints the complete reference files, recordings, model
artifacts and relevant source files. Its private model view supplies the same
corrected tokenizer to every vLLM processor without editing the original model.

`timed_speaker_metrics.py` adds pinned MeetEval tcpWER/tcpCER with an explicit
5-second collar and a separate unassigned-speaker penalty. These use synthetic
clip and predicted passage bounds, not independently validated word timings.
`summary_quality.py` separates supported statement groups from fully covered
reference facts and records owner, deadline, quantity, negation, condition,
status and topic errors. `calibrate_summary_quality.py` checks the local judge
on authored minimal pairs; it does not certify real meeting accuracy.
Short JSON verdicts retain all claim/fact IDs and evidence. Missing or invented
evidence never becomes a passing judgment; a bad quotation marks only that item
uncertain. Reports retain malformed responses, missing categories and rejected
numeric/date/owner approvals. Up to 16 independent grading requests share a
context when their estimated budgets fit; actual context errors split safely.
`statistics_report.py` reports whole-meeting/scenario bootstrap intervals,
small-group limits, missing cases, worst language combinations and paired
changes. Word and character rates are never pooled into one accuracy number.

The optional `vllm_text_helper.py` experiment uses
`requirements-text-throughput.txt` in its own `.runtime/summary-vllm-venv`.
It requires local Qwen3.5 4B Transformers weights through
`OVERTALK_VLLM_TEXT_MODEL`, never upgrades the ASR environment, and never
changes the installed desktop. It supports the zero-penalty compact extraction
and judging paths. Active sliding-window penalties are rejected explicitly.
Framework logs go to stderr while stdout remains the native JSON protocol.
This BF16 candidate differs from the GGUF model: it is not selected until
actual throughput and quality comparisons pass. Do not run it alongside the
active GPU evaluation.

The planned 50.76 speaker-hours require at least 50.76 audio-seconds/second of
sustained cloud output to fit synthesis alone in an hour. A 45-minute synthesis
budget requires 67.68 audio-seconds/second, before retries. Confirm this rate
on the actual project rather than projecting linearly from one request.
Reserve the documented maximum output in the shared cost ledger for every
in-flight request. A probe adding `generationConfig.maxOutputTokens=2048` was
rejected with HTTP 400 by the current TTS route, so the client does not assume
that a lower generation limit is supported. Reject truncated responses.

## Speech construction

Use Gemini **3.8 Flash TTS**, not Flash-Lite, with persistent, designed synthetic
voice identities. A participant retains one voice across a meeting; emotion,
pace and volume vary by turn. No real person's voice is cloned. Include calm,
uncertain, firm, frustrated, angry, quiet and relieved delivery, natural repairs,
hesitation and a small number of supported nonverbal tags.

Render speaker turns separately, then mix them at recorded offsets. This creates
real simultaneous speech, rather than a prompt merely saying people overlap.
Keep clean source audio alongside the degraded mixture. Apply deterministic
per-speaker bandwidth reduction, level changes, echo, clipping, packet loss,
dropouts and room noise. Record every transformation and seed.

Overlap is not driven by a percentage quota. Ordinary conversation
has no overlap. Only explicitly angry argument turns may interrupt each other,
late in the preceding utterance, with brief, staggered overlap (at most roughly
one second per interruption). Hard meetings have occasional arguments; really
hard meetings can have more. Do not inflate overlap to meet a percentage quota.
Measure actual overlap and its longest continuous stretch. Prompted dialect
and accent delivery must not be labelled authenticated without independent
validation.

## Reference and scoring policy

Every meeting needs a coherent agenda, explicit decisions/actions and believable
discussion. Include rejected suggestions, corrected quantities, changed dates,
reassigned tasks, genuine unresolved questions and ordinary conversational turns.
The reference contains speaker/utterance IDs, intended text, actual audio offsets,
language spans, final facts, retracted alternatives and source links.

Do not replace the reference transcript with a candidate ASR output. Mark TTS
deviations and inaudible spans separately. A fact erased by packet loss must not
silently remain an obligatory fact in the audio-grounded summary score. Human
and native-speaker review status must remain explicit.

Run Qwen3-ASR 1.7B once per audio, then reuse its output for the 4B summary
model. Also summarize the clean reference text with the same summary model
to distinguish summary errors from ASR propagation. Match the production notes
pipeline, including long-transcript windows, rather than only calling a raw LLM.

Report:

- WER and CER with declared normalization; use a script-appropriate primary
  metric and retain raw counts. Separate silence hallucination and language errors.
- Speaker-attributed cpWER/tcpWER and DER/JER when valid reference timing exists;
  declare collar, overlap policy and approximate versus real word timings.
- Final-decision coverage, rejected-proposal errors, action coverage, correct
  owner/deadline/quantity, unsupported assertions, source grounding and usability.
- Cold loading, warm real-time factor, wall time, peak process memory and VRAM.
- Failures and unsupported languages, never silently omitted from the denominator.
- Per-meeting and grouped results by difficulty, language, code-switching,
  duration, speaker count and acoustic condition, plus paired uncertainty intervals.

No single synthetic score establishes real-world superiority. Keep the full
failure examples and the evaluation policy next to any headline rating.

## Status and source boundaries

`pilot_script.json` is a hand-authored four-person English/Hindi release review.
It contains 22 turns and seven referenced facts and is **not** one of the 200
benchmark meetings. Runtime files, generated audio, cloud voice IDs, cost ledger
and predictions live under ignored `.runtime/`.

`normal_example.json`, `hard_example.json`, and `very_hard_example.json` are
additional authored listening samples. The original hard mixes were rejected
for unnatural overlap. Corrected mixes reuse the synthesized voices with
natural turn-taking and brief argument interruptions. Preserve old results for
comparison. Names added to listening samples from the synthetic reference are
display annotations after inference, never model identity predictions.

`author_corpus.py` builds the 200 controlled drafts. `render_corpus.py` uses a
shared paid-call ledger and reusable clips; it and the other TTS scripts read the
Gemini key from the file named by `--config` or `OVERTALK_TTS_ENV`; `run_evaluation.py` runs the installed
app in an isolated profile. `judge_corpus.py` adds quote-validated diagnostic
fact reviews using Qwen3.5 4B. This judge is not independent of the tested model;
invalid evidence stays ungraded. Do not describe its ratings as human ground
truth. Speech and native-language validation remain explicit limitations.

Local accuracy experiments use separate output directories and never update the
installed app or the original benchmark results:

- `probe_speaker_windows` compares uniform windows with cuts at predicted speaker
  changes. `--minimum-seconds` tests whether very short fragments hurt recognition.
- `probe_acoustic_candidates` compares frozen vocabulary and speaker-window
  candidates using an independent CTC model. Acoustic likelihood is not accuracy;
  retain per-case regressions, including rejected approaches.
- `probe_separation` tests a pinned Apache-2.0 SepFormer on artificial two-source
  mixtures. Use `--single-speaker-control` to measure unnecessary splitting.
  `probe_separation_asr` measures subsequent Qwen recognition, with source
  permutations used only for scoring. These are not natural-overlap benchmarks.
- `probe_factuality` tests a pinned, independent English NLI classifier on authored
  minimal pairs. `--pairs` accepts local `source` / `claim` records with optional
  Boolean `supported` labels. Unlabelled records never contribute to accuracy.
  Classifier probabilities are uncalibrated; neither these probes nor a
  same-model judge certify 99% factuality.
- `source_excerpt_probe --mode combined` tests an alternative that selects
  original source lines in two passes (outcomes and task commitments). Code
  renders their unchanged text; the model cannot substitute a paraphrase or
  invent an owner. Inputs are JSON lists of `{id, sources: [{id, text}]}` cases.
  Keep reference facts and phase labels out of model inputs. This is bounded to
  26 KB of source JSON and is not enabled in the app. Literal copying does not
  establish completeness, final-decision selection, correct ASR or identity.
  Both passes must parse successfully; failures remain ungraded. Output folders
  include the raw responses, source selections, hashes and probe code snapshot.

Run GPU probes under your own GPU-sharing launcher if you use one, and use a
fresh `--output` directory for each configuration. CPU probes use four threads; keep low process priority.
Save the model revision, artifact hashes, input hashes, runtime settings and
limitations with every report.

Credentials are refreshed using their actual expiry before long TTS calls. An
explicit HTTP 401 `UNAUTHENTICATED` response releases that rejected request's
reservation and is retried once with refreshed credentials; ambiguous failures
retain their full reservation. Resuming synthesis reuses cached clips. The
evaluation pins an immutable installation path and service checksum, so later
desktop installs cannot silently change the implementation halfway through.
Wall timings recorded while synthesis and development jobs share the machine
are diagnostic timings, not isolated performance benchmarks.

Primary references:

- Qwen language/dialect coverage: <https://github.com/QwenLM/Qwen3-ASR>
- Voice design: <https://docs.cloud.google.com/gemini-enterprise-agent-platform/models/text-to-speech/voice-design>
- TTS controls/formats: <https://docs.cloud.google.com/gemini-enterprise-agent-platform/models/text-to-speech/overview>
- Pricing: <https://cloud.google.com/gemini-enterprise-agent-platform/generative-ai/pricing>
- Billing spend caps: <https://docs.cloud.google.com/billing/docs/how-to/budgets-spend-caps>

### Research-driven summary comparisons

`probe_topic_notes.py` runs the real native test runner against frozen transcript
inputs using `--variant current`, `topic`, or `quoted-decisions`. The latter two
are test-only experiments, not installed-app settings. Each invocation needs a
new `--output` directory, `--native-runner`, `--helper`, `--inputs`, and explicit
`--cases`; it copies inputs and records model/helper/runner hashes. Run GPU work
under your own GPU-sharing launcher if you use one, and review the output directly. No reference answers
are supplied to generation and no local-model grades are produced.

The October 16 topic-boundary experiment was rejected: 117.47 s versus 72.25 s
for ten baseline inputs, with additional factual/coverage errors. Fresh synthetic
regression conversations and their separate review checklist are in
`frontend/src-tauri/tests/summary-context/`. The original 200-meeting corpus,
paid ledger and direct assistant reviews remain separate.
