# Word error rate on the AMI Meeting Corpus

This measures Overtalk's default built-in speech engine on real recorded meetings,
not synthetic ones. Every recording goes through the same code the app runs when a
user imports a recording, and every meeting in the standard test set is scored.

## Results

Parakeet TDT 0.6B v3 (INT8), the default engine, run through the app's audio-import path:
16 meetings, 9.06 hours of audio per condition, 89,047 reference words after normalisation.

| Condition | Pooled WER | Substitutions | Deletions | Insertions | Mean of meeting WERs | Meeting range | Real-time factor |
| --- | ---: | ---: | ---: | ---: | ---: | --- | ---: |
| headset (close-talk mix) | **34.80%** | 3.93% | 29.86% | 1.00% | 33.73% | 17.50% (IS1009c) – 46.86% (EN2002a) | 0.082 |
| farfield (one table mic) | **38.33%** | 5.77% | 31.54% | 1.02% | 37.00% | 15.40% (IS1009c) – 56.55% (TS3003a) | 0.081 |

Rates are shares of all reference words. The real-time factor is wall time divided by audio
length (0.082 means a one-hour meeting took about 5 minutes) on the CPU, with other heavy jobs
running at the same time (see Limits).

**Most errors are words that are missing from the transcript, not words recognised wrongly.**
Where a word does appear, it is usually right: substitutions are 3.9% (headset) and 5.8%
(far field). Deletions by the kind of utterance a reference word belongs to:

| Utterance kind | headset: share of words | headset: deleted | farfield: share of words | farfield: deleted |
| --- | ---: | ---: | ---: | ---: |
| clear: nobody else speaks during more than 20% of it | 55.9% | 13.5% | 50.3% | 12.5% |
| overlapped: another participant speaks during more than 20% of it | 42.1% | 49.1% | 39.9% | 45.4% |
| no saved row: outside every transcript row the app saved | 1.9% | 83.7% | 9.8% | 72.8% |

When people talk over each other, a single-channel transcript usually keeps one voice and
the other voice's words count as deletions. Overlapped utterances hold 69% of all deletions
on the headset mix and 58% in the far field. In the far field, another 10% of the reference
words lie outside every transcript row the app saved. Either the VAD dropped that audio, or
the model returned no text for that chunk; the app saves no row in either case, so the two
cannot be told apart from the saved transcript.

Even on clear speech, 12–14% of words are missing. Some of this comes from the model itself:
in some chunks it returns far less than was said. For example, in IS1009a (headset) the
20.3 s chunk at 393.8 s comes back only as "Okay.". Sent to the same worker as three
consecutive windows (393.8–401.3, 401.3–408.8 and 408.8–414.1 s), the same audio returns
"Okay that should keep everybody on their toes and challenged." / "So these are all things
of course." / "". That is still incomplete, but it is 16 words instead of 1. To reproduce
it, send those samples (16 kHz float32) to
`sonefold-service --native-model-worker parakeet <models>/parakeet parakeet-tdt-0.6b-v3-int8`.
That process reads a JSON header line `{"operation":"transcribe","samples":N,"language":null}`
followed by the raw samples.

### Diagnostic: shorter chunks are not the fix

To test whether the 25 s chunks cause the missing words, the headset condition was run again
with one constant changed: `MAX_SEGMENT_SAMPLES` in `audio::common::prepare_speech_segments`,
from `25 * 16000` to `10 * 16000`. This was a diagnostic build only; the change was not
committed, and the binary SHA-256 was `bce3d30213580bcd42433e61859b034052e2e8e2eb056d5ebb4b49eb7176e7d5`.
With 10 s chunks, pooled WER was **35.58%** (S 4.01%, D 30.54%, I 1.03%), against 34.80% with
the shipped 25 s. It helped 6 of the 16 meetings, most of all IS1009a (45.64% → 33.11%), and
made the other 10 worse, for example IS1009b (20.79% → 25.86%). More chunks came back empty:
words with no saved row went from 1.9% to 4.1%. The full scoring is in `diagnostic-max-chunk-10s.json`.
These are not the app's figures.

### Per meeting

Columns S, D and I are counts. "WER, fragments removed" drops AMI's truncated word fragments
from the reference. RTF is the real-time factor.

**headset**

| Meeting | Ref. words | WER | S | D | I | WER, fragments removed | RTF |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| EN2002a | 7791 | 46.86% | 358 | 3240 | 53 | 46.39% | 0.0978 |
| EN2002b | 6208 | 44.46% | 336 | 2354 | 70 | 44.07% | 0.0877 |
| EN2002c | 10945 | 40.52% | 579 | 3688 | 168 | 40.01% | 0.0808 |
| EN2002d | 8029 | 46.63% | 420 | 3256 | 68 | 45.95% | 0.0980 |
| ES2004a | 2669 | 34.28% | 81 | 823 | 11 | 33.61% | 0.0689 |
| ES2004b | 6779 | 26.97% | 198 | 1563 | 67 | 26.59% | 0.0767 |
| ES2004c | 7067 | 28.07% | 267 | 1633 | 84 | 27.47% | 0.0794 |
| ES2004d | 6274 | 34.25% | 210 | 1901 | 38 | 33.75% | 0.0786 |
| IS1009a | 1948 | 45.64% | 44 | 827 | 18 | 45.12% | 0.0716 |
| IS1009b | 5954 | 20.79% | 161 | 1011 | 66 | 20.57% | 0.0786 |
| IS1009c | 4222 | 17.50% | 128 | 585 | 26 | 16.76% | 0.0751 |
| IS1009d | 4551 | 27.71% | 103 | 1112 | 46 | 27.36% | 0.0757 |
| TS3003a | 2511 | 27.36% | 89 | 572 | 26 | 27.21% | 0.0994 |
| TS3003b | 4627 | 24.36% | 179 | 893 | 55 | 23.99% | 0.0877 |
| TS3003c | 4287 | 30.39% | 148 | 1097 | 58 | 29.88% | 0.0731 |
| TS3003d | 5185 | 43.90% | 200 | 2036 | 40 | 43.39% | 0.0824 |

**farfield**

| Meeting | Ref. words | WER | S | D | I | WER, fragments removed | RTF |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| EN2002a | 7791 | 49.26% | 508 | 3292 | 38 | 48.82% | 0.1015 |
| EN2002b | 6208 | 53.30% | 433 | 2840 | 36 | 52.77% | 0.0870 |
| EN2002c | 10945 | 46.97% | 715 | 4234 | 192 | 46.45% | 0.0951 |
| EN2002d | 8029 | 52.77% | 526 | 3667 | 44 | 52.15% | 0.0767 |
| ES2004a | 2669 | 41.10% | 196 | 878 | 23 | 40.52% | 0.0709 |
| ES2004b | 6779 | 25.77% | 385 | 1268 | 94 | 25.33% | 0.0965 |
| ES2004c | 7067 | 29.43% | 442 | 1532 | 106 | 28.68% | 0.0778 |
| ES2004d | 6274 | 44.23% | 358 | 2374 | 43 | 43.68% | 0.0782 |
| IS1009a | 1948 | 29.21% | 115 | 431 | 23 | 28.64% | 0.0858 |
| IS1009b | 5954 | 19.10% | 213 | 837 | 87 | 18.80% | 0.0954 |
| IS1009c | 4222 | 15.40% | 188 | 440 | 22 | 14.33% | 0.0743 |
| IS1009d | 4551 | 21.75% | 188 | 754 | 48 | 21.14% | 0.0837 |
| TS3003a | 2511 | 56.55% | 111 | 1300 | 9 | 56.39% | 0.0462 |
| TS3003b | 4627 | 29.26% | 242 | 1065 | 47 | 28.63% | 0.0804 |
| TS3003c | 4287 | 28.46% | 236 | 936 | 48 | 27.72% | 0.0652 |
| TS3003d | 5185 | 49.45% | 280 | 2238 | 46 | 48.94% | 0.0661 |

`results.json` holds every figure above plus the run provenance and the load averages.
In the recorded commands, `$OVERTALK_EVAL_CACHE` stands for the data directory
(`~/.cache/overtalk-eval` by default) and `$SCRATCH` for a disposable working directory.

## What was measured

| | |
| --- | --- |
| Corpus | AMI Meeting Corpus, University of Edinburgh et al., CC BY 4.0. <https://groups.inf.ed.ac.uk/ami/corpus/> |
| Meetings | The 16 meetings of the standard AMI full-corpus ASR test set: EN2002a–d, ES2004a–d, IS1009a–d, TS3003a–d. None left out. |
| Conditions | `headset`: `<meeting>.Mix-Headset.wav`, the sum of every participant's close-talk headset microphone in one channel. `farfield`: `<meeting>.Array1-01.wav`, one omnidirectional microphone of the table array (a single distant microphone, no beamforming). Both 16 kHz mono. |
| Reference | `ami_public_manual_1.6.2.zip`, NXT word layer (`words/*.words.xml`) and segment layer (`segments/*.segments.xml`). |
| Engine | Parakeet TDT 0.6B v3, INT8 ONNX (`parakeet-tdt-0.6b-v3-int8`, `crate::config::DEFAULT_PARAKEET_MODEL`), conversion revision `8f23f0c03c8761650bdb5b40aaf3e40d2c15f1ce`. Downloaded by the app's own downloader into a disposable profile; all four file digests match the app's pinned SHA-256 values. ONNX Runtime 1.22.0 through the `ort` crate 2.0.0-rc.10, `CPUExecutionProvider`, inside the app's isolated native inference worker. |
| Code | Commit `d27d780a696ab5baf251df0ee33fca707e151028` (clean tree), release `sonefold-service` built with rustc 1.94.0, SHA-256 `35ab66a49941ff1e0460570f4233760c3406015eddd544695371426894e2dcd0`. A second build from the same commit produced the same bytes. The application code is that of `a6fe6cc`; `d27d780` adds only the harness. Both commit IDs refer to the project's earlier, unpublished history. The scores were computed with `reference.py` and `score.py` as committed next to `results.json`; that version adds the utterance-kind breakdown. FFmpeg 6.1.1 (Ubuntu package). |
| Hardware | Intel Core Ultra 9 275HX (24 logical CPUs), Linux 7.0. This engine runs on the CPU only (`CPUExecutionProvider`), so the RTX 5090 Laptop GPU is not used. |
| Date | 2026-10-20 to 2026-10-21 (one run of about 90 minutes) |

### The application path

`run_overtalk.py` starts a release `sonefold-service --stdio` with `XDG_DATA_HOME`,
`XDG_CONFIG_HOME` and `XDG_CACHE_HOME` inside a throwaway directory, points the
recordings folder there, and calls `start_import_audio_command` with provider
`parakeet`. That is `audio::import::run_import`:

1. FFmpeg (sandboxed) decodes the file; it is converted to 16 kHz mono.
2. Silero VAD splits it into speech segments (thresholds 0.50/0.35, 2 s redemption,
   300 ms/400 ms padding, 250 ms minimum).
3. Segments longer than 25 s are split at the quietest 100 ms within ±3 s of each
   25 s mark (`audio::common::prepare_speech_segments`); segments under 0.1 s are skipped.
4. Each segment is sent to the Parakeet worker; the text is stored as a transcript row.

The harness then reads every row back with `api_get_meeting_transcripts`. No
transcript text is changed after the app produces it.

## Scoring

* **Reference words**: every `<w>` element of every participant except punctuation
  (`punc="true"`). Noise and event elements (`vocalsound`, `gap`, `disfmarker`, …) are
  not words and are excluded. Truncated fragments (`trunc="true"`, e.g. "compo") stay in
  the reference; the table also shows the rate with them removed. AMI writes spelled
  acronyms letter by letter ("T_V_", "L_C_D_s"); these are joined ("TV", "LCDs") before
  normalisation, because a transcript writes them that way.
* **Order**: AMI segments (utterances) of all participants sorted by start time; words keep
  their order inside an utterance. Hypothesis: the app's transcript rows sorted by start time.
* **Normalisation**, applied the same way to both sides, per reference utterance and per
  hypothesis row: Whisper's `EnglishTextNormalizer` from openai-whisper 20250625 (MIT),
  vendored in `vendor/whisper_normalizer/`. It lowercases, removes bracketed text, removes
  the fillers *hmm, mm, mhm, mmm, uh, um*, expands contractions, turns spelled numbers into
  digits ("twenty five" → "25"), maps British to American spelling, and removes punctuation
  and symbols. Two import lines were changed so it needs no third-party packages:
  `more_itertools.windowed` is replaced by an equivalent local function, and `regex` is
  imported only by the unused `split_letters` option. The other code and `english.json`
  are byte-identical to the wheel (checked against its `RECORD` hashes).
* **Metric**: one Levenshtein alignment over the whole meeting (substitution, deletion and
  insertion each cost 1; ties prefer a substitution, then a deletion). WER = (S + D + I) /
  reference words. The pooled figure is total errors over total reference words across the
  16 meetings, not a mean of meeting WERs. The total edit count is cross-checked against the
  independent bit-parallel distance in `evaluations/meetingbench/metrics.py`.
* **Where the errors are**: `reference.py` records, for each AMI utterance, the share of its
  span during which other participants are speaking words (from AMI's word timings). The
  scorer follows the alignment back to each reference word. It counts a word as *no saved row*
  when its utterance's midpoint falls outside every transcript row the app saved (dropped by
  the VAD, or a chunk the model returned empty), as
  *overlapped* when that share is above 20%, and as *clear* otherwise. Insertions have no
  reference word, so they are not split this way.
* **Real-time factor**: wall time from the import call to the completion event (decode,
  VAD, model load, inference and saving), divided by the recording length. The model is
  loaded for every import, as in the app. Other jobs shared the machine; the 1/5/15-minute
  load averages before and after each meeting are in `results.json`.

## Reproduce

```bash
# 1. Data (about 2.2 GB; nothing is committed): the annotations zip and the 32 wav files
#    from the AMI mirror into ~/.cache/overtalk-eval/ami/. Repeat it if a file failed.
evaluations/ami/fetch.sh
# 2. References (the archive is untrusted input: isolated mode, no extraction)
python3 -I evaluations/ami/reference.py \
  --archive ~/.cache/overtalk-eval/ami/annotations/ami_public_manual_1.6.2.zip \
  --output /tmp/ami/references
# 3. Build the service (needs the sandbox worker and sidecars staged as for any build)
python3 scripts/worker_build.py
cargo build --release -p sonefold --bin sonefold-service
# 4. Transcribe (resumable; downloads the model into the disposable profile if needed)
python3 evaluations/ami/run_overtalk.py --binary target/release/sonefold-service \
  --audio-dir ~/.cache/overtalk-eval/ami/audio --profile /tmp/ami/profile --output /tmp/ami/hyp
# 5. Score
python3 -I evaluations/ami/score.py --references /tmp/ami/references \
  --hypotheses /tmp/ami/hyp --output evaluations/ami/results.json
```

Tests: `python3 -m unittest discover -s evaluations/ami -t .`

## Limits

* English only, and meetings recorded in 2005. Many speakers are non-native; ES, IS and TS
  meetings are role-played design scenarios, EN2002 meetings are natural.
* `headset` is cleaner than what a laptop hears: every speaker is close to a microphone.
  `farfield` is one table microphone without beamforming, echo cancellation or the system
  audio channel a real Overtalk recording has. Neither is the app's two-channel live path.
* One alignment per meeting against a single-channel transcript: when people talk over each
  other, the words of all but one speaker usually count as deletions. Speaker attribution is
  not scored.
* The figures are not comparable with segment-level leaderboard numbers for AMI, which feed
  the model one human-segmented utterance from one speaker's headset at a time.
* Whisper's normaliser has known quirks that apply to both sides (for example "oh" becomes
  "0"). Truncated fragments count as reference words.
* The real-time factor is from a shared workstation, not an idle benchmark machine: the
  one-minute load average was 15–46 on 24 logical CPUs during the run (the engine itself
  uses about 19 cores). It includes decoding, VAD and loading the model for every meeting.
* The 20% overlap threshold and the "clear / overlapped / no saved row" split are a diagnostic
  view chosen for this report, not a standard AMI metric. The pooled WER does not depend on it.
* Qwen3-ASR 1.7B (the optional speech worker) was not measured. The app has no downloader
  for its checkpoint and Python environment (it loads with `local_files_only`), and the
  only installed copy belonged to a real, in-use Overtalk library, which this evaluation
  does not touch. The `-hf` checkpoint in the Hugging
  Face cache is a different format from the one the worker loads.
