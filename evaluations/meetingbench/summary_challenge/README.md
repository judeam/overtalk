# Meeting-summary challenge v1

Eighteen new, hand-authored synthetic conversations in 18 distinct domains.
Unlike the 200-meeting audio set, they do not share one agenda template. These
are text-only diagnostics: they establish neither speech-recognition accuracy
nor real-world meeting performance. The original audio set remains separate.

The six development cases, six validation cases and six sealed holdout cases
each contain two normal, two hard and two very-hard conversations. Languages:
English, German, English/Hindi, Spanish, French, Chinese, Korean, Portuguese,
Japanese, Italian and Arabic. The holdout adds a noisy transcript with a truly
unknown name, and a multi-project Chinese discussion. The transcripts cover
completed versus future work, proposals versus decisions, changing constraints,
numeric scope, deadlines attached to different verbs, project isolation,
consent, causal uncertainty, and quoted prompt injection.

Freeze all inputs and rubric hashes before inference:

```sh
python3 -m evaluations.meetingbench.summary_challenge prepare --output /tmp/frozen-summary-challenge
```

Only `transcript` and `language` enter the native inference harness. Expected
facts, forbidden inferences, difficulty and split are not model inputs. The
runner rejects changed inputs/rubrics and existing output directories. All
cloud calls go through the confirmed-Free-plan token-reserving local gateway.
No synthetic audio is generated or paid Gemini allowance consumed.

## Grading contract

An AI coding assistant reviews every report directly against the transcript;
the harness has no automated local or cloud judge model. Each required fact is covered, partial, missing or
contradicted. Each substantive report line (including metadata, quotes and
action-table rows) is inventoried and judged against cited evidence. A line
with one wrong assertion fails even when its other assertions are correct.
Every supported claim must carry source-line evidence in the review artifact.
Review artifacts bind to report hashes; incomplete or stale reviews fail closed.

A case passes only if all required facts are fully covered, all emitted claims
are supported, and no citation errors remain. Abstaining on unknown ownership
is correct; omitting a recoverable owner is incomplete. A perfect fact-recall
score with invented actions is a failure. Empty output and generation errors
fail. Unreviewed cases remain in the denominator, never silently excluded.

Report separate coverage, factual support, attribution/citation failures,
all-or-nothing case success, per-language/difficulty outcomes, generation
latency, token use and generation failures. Do not claim population-wide 100%
from a finite synthetic challenge; 18/18 successes would still leave substantial
uncertainty. Repeated attempts on a holdout convert it into development data;
label that explicitly and create a new holdout before making a new claim.

## Experiment protocol

1. Freeze and baseline all models before changing the pipeline. Keep previous
   long/ASR comparisons intact as separate regression controls.
2. Tune only against development results. Use validation for candidate selection.
3. Lock a candidate binary and prompts before opening holdout outputs.
4. Compare paired inputs for GLM-5.3-Flash, GLM-5.3, GPT-OSS-120B and Qwen3.5-4B.
   Other model prompt/dispatch paths must remain byte-identical if untouched.
5. Reject quality regressions or material latency increases. Record raw timings
   and schedule: cloud runs are serial and local runs use four shared workers,
   so those timings are not an isolated hardware comparison.

This suite supplements the original long and ASR-corrupted recordings. It does
not replace them or erase their failures. Initial baseline and subsequent
candidates live under ignored `.runtime/experiments/` directories.
