# Improving local multilingual meeting accuracy — 15 October 2026

## Decision and evidence boundary

The highest-priority experiment is **speaker-guided retranscription of currently unaligned, multi-speaker windows**, followed by an explicit language-specific alignment fallback. Keep Qwen3-ASR 1.7B, Nemotron-3, CAMPPlus and Qwen3.5 4B as the baseline. Fixing text-to-speaker reconciliation is more directly connected to the observed failure than replacing the diarizer or increasing the summary model size.

This is a research note, not a measured improvement or a claim of 99% performance. The primary sources below establish available methods, interfaces and limitations; the proposed thresholds, routing and experiment order are engineering recommendations requiring local evaluation. No models, production code, evaluation results, references or Git metadata were changed for this note.

The cutoff is **2026-10-15**. Sources are first-party repositories/model cards, official documentation and research papers published by that date. Mutable documentation was inspected on this date; historical model revisions already recorded in this repository are listed below. A current `main` URL is not an immutable deployment specification. No result dated after the cutoff is used.

Preserve the existing [200-meeting protocol](../../evaluations/meetingbench/README.md), its **100 normal / 60 hard / 40 really hard** allocation, original scripts/audio/references, immutable baseline results and fixed paid-synthesis budget. These recommendations do not require more paid synthesis, replacement of references with ASR output, or rewriting baseline results. The protocol already requires distinguishing authored text from what the synthesizer actually uttered.

## What the baseline implementation establishes

These observations describe **baseline commit `ab5c9a0`** (in the project's earlier, unpublished history), before the subsequent meeting-accuracy changes. Read-only inspection of its ASR, diarization, worker output handling and worker documentation found:

- Audio is decoded in at-most-30-second windows, preferably ending near quiet audio. Qwen can receive a language hint and up to 4,000 characters of vocabulary context. Repetition triggers shorter redecoding.
- Word alignment is attempted only for the supported Qwen aligner languages. Missing/failed alignment preserves text with approximate chunk timing; it does not invent word times. Collapsed aligned words can be retained in timed phrases.
- Speaker reconciliation uses interval support and a margin; ambiguous segments abstain. An unaligned window containing several speakers therefore may remain unassigned even when the diarization turns are correct. Assigning its whole text to the longest speaker would conceal this uncertainty.
- Nemotron settings are `spkcache=264, fifo=40, chunk=340, right=40, update=300`. These match NVIDIA's recommended **30.4-second offline input-buffer** configuration. Buffer latency excludes computation. It is not evidence that the 30-second ASR window should receive one speaker label. [Nemotron model card](https://huggingface.co/nvidia/Nemotron-3-Diarization)

Qwen documents **30 languages plus 22 Chinese dialects/varieties** for ASR, and 11 aligner languages: Chinese, English, Cantonese, French, German, Italian, Japanese, Korean, Portuguese, Russian and Spanish. These are different capability sets. Hindi, Arabic, Thai, Vietnamese and the other unsupported aligner languages need a deliberate route. [Qwen repository](https://github.com/QwenLM/Qwen3-ASR)

## Define the three 99% targets separately

| Target | Primary measurement | Required companion checks |
| --- | --- | --- |
| Transcript fidelity | WER ≤1% for whitespace-tokenized languages; a declared script-appropriate CER/token metric for other languages | Raw substitutions/deletions/insertions, names/numbers/negation errors, omissions, hallucinated silence, language and code-switching errors. `1-WER` is only a convenient presentation, not a bounded universal accuracy measure. |
| Speaker assignment | Fraction of reference lexical units both recovered and assigned to the correct anonymous speaker, with unknown/deleted units counted as failures | Also report attribution conditional on correctly recognized units, anonymous-cluster mapping rules, cpWER/tcpWER, DER/JER, overlap and speaker-count errors. Keep named-person recognition separate from anonymous diarization. |
| Summary factuality | Supported atomic claims / all generated atomic claims, target ≥99% | Critical-fact recall, correct action owner/deadline/quantity, contradiction/retraction handling, source-citation support and abstention rate. An empty summary must not pass by omitting every fact. |

The summary metric adapts the atomic-claim idea in FActScore; its published evaluation concerns biographies, so its reported scores do not transfer to these meetings. [FActScore, EMNLP 2023](https://aclanthology.org/2023.emnlp-main.741/)

Report **all three targets in each of the three difficulty groups**, plus language, duration, speaker-count and overlap strata. Declare normalization before comparing runs, retain unnormalized outputs, and include failures in denominators. Use paired, meeting-level uncertainty intervals because words in one meeting are correlated. Publish micro and per-meeting macro results so long easy meetings cannot conceal short hard failures. A point estimate on this synthetic corpus would not establish 99% on arbitrary real meetings.

Use DER with a stated collar and overlap policy. The baseline meetingbench `interval_diagnostics` uses a **0.25-second reference-boundary collar**. A separate zero-collar, overlap-inclusive score is recommended for stricter comparison; it is not the implemented baseline metric. DIHARD explicitly tests diverse acoustic domains and separates oracle speech activity from processing raw audio. These are materially different evaluation conditions. [DIHARD III paper](https://www.isca-archive.org/interspeech_2021/ryant21_interspeech.html)

MeetEval's tcpWER adds temporal constraints to permutation-based word scoring, and warns that timing approximation/collar choices affect scores. Its synthetic per-word times are **scoring machinery**, not grounds to claim acoustically aligned word timestamps in the product. [MeetEval tcpWER documentation](https://github.com/fgnt/meeteval/blob/main/doc/tcpwer.md)

### Chinese script differences require a separate diagnostic

The reported Cantonese example differs in script (`講`/`讲`, `項`/`项`) while retaining Cantonese words such as `我哋`. NFKC leaves those Traditional/Simplified pairs distinct. Consequently, the baseline CER includes orthographic differences that do not by themselves establish an acoustic recognition error. It remains a valid measurement of the original reference's requested written form.

**Evidence:** script folding is a documented ASR evaluation practice, not a universal standard. MSR-86K's Interspeech 2024 evaluation explicitly converts Traditional Chinese to Simplified before Chinese scoring. By contrast, Whisper's published basic non-English normalizer applies NFKC, punctuation/whitespace processing and character scoring without that conversion; the paper acknowledges imperfect normalization. Therefore comparisons must name the exact policy. [MSR-86K evaluation](https://www.isca-archive.org/interspeech_2024/li24s_interspeech.pdf), [Whisper, Appendix C](https://cdn.openai.com/papers/whisper.pdf)

**Recommendation:** preserve all baseline scores, references and hypotheses. Add only a clearly labeled **script-canonicalized CER diagnostic**: apply the same pinned OpenCC `t2s` configuration to temporary copies of both sides, then the existing normalization/scorer. Report raw CER alongside it and audit changed pairs. Avoid regional phrase-conversion profiles such as `s2twp`, which can change vocabulary. Folding is lossy and can conceal real distinctions, so it cannot establish 99% transcription accuracy alone.

OpenCC is Apache-2.0 and distinguishes character, phrase and regional-variant conversion; it explicitly does **not** translate between Mandarin and Cantonese. Its dictionaries are not proof of correct colloquial Cantonese spelling. One reproducible pre-cutoff option is **1.1.9**, published 2024-08-08, tag commit `556ed22496d650bd0b13b6c163be9814637970ae`; record the actual library and dictionary hashes, and review Cantonese-specific changes. [Pinned README](https://github.com/BYVoid/OpenCC/blob/ver.1.1.9/README.md), [release metadata](https://pypi.org/project/OpenCC/1.1.9/)

Qwen's language validator includes `Chinese` and `Cantonese`, but no documented `zh-Hant`, `zh-Hans` or script-preference field. Its `context` input permits an experiment with Traditional-script vocabulary/examples, but the inspected API gives no guarantee of script control. Preserve `language="Cantonese"` for genuinely monolingual Cantonese tests; do not substitute Mandarin to obtain a preferred script. Evaluate any hint separately on held-out audio. [Language validator](https://github.com/QwenLM/Qwen3-ASR/blob/main/qwen_asr/inference/utils.py), [context API](https://github.com/QwenLM/Qwen3-ASR/blob/main/qwen_asr/inference/qwen3_asr.py)

## 1. Resolve unsupported alignment without fabricated timestamps

**Recommendation: use a two-pass route only for affected regions.**

1. Run full-recording diarization and retain all regular overlapping turns. Keep the existing ASR output as an immutable candidate.
2. Identify an approximate-timing window intersecting multiple speakers. Build contiguous **single-active-speaker** intervals from diarization, intersected with actual speech activity. Treat concurrent-speaker intervals separately.
3. Retranscribe those single-speaker intervals with Qwen, using small real-audio context margins and absolute sample offsets. Start the boundary-padding sweep around 150–300 ms; this is a proposed search range, not a validated optimum. Prefer a nearby silence when possible.
4. Retain longer context for very short backchannels by evaluating a padded candidate; do not stretch its acoustic ownership across another speaker's speech. Mark unresolved fragments unknown. Deduplicate overlapping retries through text alignment plus actual time constraints, preserving each word once.
5. Assign a fragment only when its **speech-bearing core** belongs confidently to one diarized speaker. Preserve segment/phrase timing when no word aligner exists. Do not spread its words uniformly across time or advertise word precision.
6. Compare retranscription with the immutable original for omissions, boundary-word loss, duplicated words and unwanted language changes. Retain both candidates and provenance until selection criteria are validated.

This route removes the need to guess where every word fell inside a 30-second mixed window. It can still fail if diarization boundaries are wrong, if cropping removes linguistic context, or if true overlap dominates. The general use of speech segmentation and acoustic alignment has published support, but WhisperX's results are not a measured result for Qwen or this routing scheme. [WhisperX, Interspeech 2023](https://www.isca-archive.org/interspeech_2023/bain23_interspeech.pdf)

### Alignment fallback choices

| Candidate | Evidence and license | Recommendation |
| --- | --- | --- |
| Existing Qwen forced aligner | Apache-2.0; 11 documented languages; independent aligner checkpoint. [Qwen model card](https://huggingface.co/Qwen/Qwen3-ForcedAligner-0.6B) | Keep for supported-language spans. Route language switches and failures explicitly rather than assuming the ASR language list applies. |
| Meta omniASR-CTC-300M | Apache-2.0 model/code; shared multilingual character tokenizer and exposed CTC logits. [Model card](https://huggingface.co/facebook/omniASR-CTC-300M) | First broad-coverage permissive alignment candidate to prototype while retaining Qwen text; see the version/tokenizer and deployment caveats below. |
| Language-specific CTC model through NeMo Forced Aligner | Accepts supplied text; requires CTC or the CTC head of a hybrid model. Pure transducer checkpoints do not work. Local checkpoint loading is supported. [NFA documentation](https://docs.nvidia.com/nemo-framework/user-guide/latest/nemotoolkit/tools/nemo_forced_aligner.html) | Preferred extensible fallback when a suitable permissively licensed acoustic model exists. Maintain a per-language checkpoint/tokenizer/license/quality manifest; an Apache toolkit does not establish every checkpoint's license. |
| Whisper alignment through `stable-ts` | MIT code; accepts supplied text and offers alignment-failure controls. The repository was archived on **2026-05-30**. [Repository](https://github.com/jianfch/stable-ts), [archive status](https://github.com/jianfch/stable-ts/releases) | A useful local research baseline for gaps, with a pinned compatible runtime. Budget another model pass and maintenance ownership. Do not assume arbitrary unsupported-language text becomes accurately aligned. |
| MMS / `ctc-forced-aligner` | Broad multilingual alignment; BSD-2-Clause wrapper, but its default MMS-derived weights are **CC-BY-NC-4.0**. Romanization is required for its default model. [Maintainer repository](https://github.com/MahmoudAshraf97/ctc-forced-aligner) | Do not make the default weights a commercially distributable fallback. The wrapper can use other compatible licensed models, evaluated individually. |

### Follow-up: omniASR as a shared permissive CTC aligner

**Verdict:** a practical first candidate for replacing the *licensing-restricted MMS alignment dependency*, while keeping Qwen's transcription. It is not yet a validated all-language forced aligner. Meta publishes its model/code under Apache-2.0; the original checkpoint history dates to **2025-11-27**, and the source declares library version **0.2.0**. No weights/tokenizers were downloaded or inference run for this review. [License](https://github.com/facebookresearch/omnilingual-asr/blob/main/LICENSE), [checkpoint history](https://huggingface.co/facebook/omniASR-CTC-300M/commits/main), [library version](https://github.com/facebookresearch/omnilingual-asr/blob/main/src/omnilingual_asr/__init__.py)

**Coverage:** the published language/script list contains entries corresponding to all Qwen30. In Qwen order, the mapping is `zh→cmn_Hans/cmn_Hant`, `en→eng_Latn`, `yue→yue_Hans/yue_Hant`, `ar→arb_Arab`, `de→deu_Latn`, `fr→fra_Latn`, `es→spa_Latn`, `pt→por_Latn`, `id→ind_Latn`, `it→ita_Latn`, `ko→kor_Hang`, `ru→rus_Cyrl`, `th→tha_Thai`, `vi→vie_Latn`, `ja→jpn_Jpan`, `tr→tur_Latn`, `hi→hin_Deva`, `ms→zsm_Latn`, `nl→nld_Latn`, `sv→swe_Latn`, `da→dan_Latn`, `fi→fin_Latn`, `pl→pol_Latn`, `cs→ces_Latn`, `fil→fil_Latn`, `fa→fas_Arab`, `el→ell_Grek`, `hu→hun_Latn`, `mk→mkd_Cyrl`, `ro→ron_Latn`. This verifies published coverage, not token coverage for every name/symbol or accurate alignment in every dialect. [Official language list](https://github.com/facebookresearch/omnilingual-asr/blob/main/src/omnilingual_asr/models/wav2vec2_llama/lang_ids.py)

**Tokenizer contract:** v1 uses `omniASR_tokenizer_v1` and **9,812** output classes; v2 uses `omniASR_tokenizer_written_v2` and **10,288**. Both are registered as `char_tokenizer`; fairseq2 implements this through `RawSentencePieceTokenizer`, without added control symbols. Use the checkpoint's paired tokenizer, not Qwen tokens, a hand-written Unicode mapping or the other Omni generation. Check round-trip/unknown tokens for the exact original scripts, names, digits and punctuation before aligning; retain a reversible normalized-text mapping. [V1 assets](https://github.com/facebookresearch/omnilingual-asr/blob/main/src/omnilingual_asr/cards/models/rc_models_v1.yaml), [v2 assets](https://github.com/facebookresearch/omnilingual-asr/blob/main/src/omnilingual_asr/cards/models/rc_models_v2.yaml), [vocabulary configuration](https://github.com/facebookresearch/omnilingual-asr/blob/main/src/omnilingual_asr/models/wav2vec2_asr/config.py), [tokenizer implementation](https://github.com/facebookresearch/fairseq2/blob/main/src/fairseq2/data/tokenizers/char.py)

**Emission/timing contract:** the official pipeline calls `logits, output_layout = model(waveforms, input_layout)`, then argmax-decodes. An alignment adapter should instead retain the logits, valid output lengths and matching tokenizer, compute float32 log-softmax, and run CTC forced alignment against Qwen text. `transcribe()` returns strings; it is not an alignment API. CTC ignores the pipeline's `lang` argument and does not support its context-example path. The 16 kHz encoder downsamples by 320, giving a **20 ms nominal frame stride**; this is resolution, not guaranteed boundary accuracy. Use actual feature lengths/receptive-field offsets and trim padded frames. [Inference source](https://github.com/facebookresearch/omnilingual-asr/blob/main/src/omnilingual_asr/models/inference/pipeline.py), [architecture](https://github.com/facebookresearch/omnilingual-asr/blob/main/src/omnilingual_asr/models/README.md), [fairseq2 CTC implementation](https://github.com/facebookresearch/fairseq2/blob/main/src/fairseq2/models/wav2vec2/asr/model.py)

**Cost/deployment:** Meta lists 325,494,996 parameters, 1.3 GiB FP32 download and approximately 2 GiB inference VRAM for a 30-second, batch-one BF16 A100 run. That is not a CPU RAM or laptop-throughput guarantee. A 30-second FP32 emission tensor alone is roughly 56–59 MiB, calculated from 1,500 frames and the two vocabulary sizes; weights, activations and alignment workspace are additional. The official runtime supports an explicit CPU device and dtype; use float32 in an isolated environment. Source `pyproject.toml` permits Python 3.10–3.13, but the **published 0.2.0 package declares `>=3.10,<=3.12`**, which excludes patch releases above 3.12.0. Use Python 3.11; do not bypass the metadata or alter the Qwen environment. Its ordinary pipeline caps audio at 40 seconds, so current 30-second windows fit. Use local asset cards to prevent runtime downloads. [Model specifications](https://huggingface.co/facebook/omniASR-CTC-300M), [source dependencies](https://github.com/facebookresearch/omnilingual-asr/blob/main/pyproject.toml), [published package metadata](https://pypi.org/pypi/omnilingual-asr/0.2.0/json), [local assets](https://github.com/facebookresearch/omnilingual-asr/blob/main/src/omnilingual_asr/cards/README.md)

**Format availability:** the inspected Meta distribution contains `.pt` weights and the tokenizer; its reference runtime is fairseq2. No Meta-published ONNX/Transformers checkpoint was found in those sources. An [independent ONNX conversion](https://huggingface.co/EmreAkgul/omniASR-CTC-300M-ONNX) explicitly disclaims Meta affiliation, and an [HF-staff Transformers prototype](https://huggingface.co/bezzam/omniasr-ctc-300m-v2) has an incomplete model card. These are candidates for a separately verified conversion, not evidence of official supported deployment. In particular, the ONNX card names original v1 but advertises 10,288 logits: resolve version/tokenizer provenance and compare FP32 logits before trusting it. Avoid conversion/quantization until the original-runtime alignment route passes the language and timing checks.

#### Isolated CPU prototype compatibility

Do not install this into the existing Qwen/Nemotron environment. **Linux x86_64, CPython 3.11, PyTorch 2.8.0 CPU and fairseq2/fairseq2n 0.6** satisfy the published compatibility constraints. PyPI normalizes `0.6.0` to `0.6`; `fairseq2==0.6` requires `fairseq2n==0.6`, whose base metadata requires `torch==2.8.0` and `tbb>=2021.8` on x86_64. The working native CPU wheel is `fairseq2n==0.6+cpu` from Meta's index. Its Linux wheels require glibc 2.28 or later. fairseq2 explicitly warns that its PyTorch C++ ABI must match exactly. [Native-package metadata](https://pypi.org/pypi/fairseq2n/0.6.0/json), [Python-package metadata](https://pypi.org/pypi/fairseq2/0.6.0/json), [versioned compatibility matrix](https://github.com/facebookresearch/fairseq2/blob/v0.6.0/README.md)

The implementation session subsequently reported a working isolated setup with the following pins. This is **not a complete dependency lock**. Install Torch/Torchaudio from the PyTorch CPU index and native fairseq2 from Meta's `pt2.8.0/cpu` index; lock the remaining resolved wheels/hashes. Pinning PyArrow 20 avoids the observed newer-PyArrow/NumPy mismatch with this older fairseq2 stack. `libsndfile` is a system dependency. KenLM required a local GCC 13 build with `LIBRARY_PATH`/`LDFLAGS` pointing to `/usr/lib/gcc/x86_64-linux-gnu/13`; that is an observed machine-specific workaround, not a universal requirement. [PyTorch 2.8 CPU installation](https://pytorch.org/get-started/previous-versions/), [fairseq2 installation](https://github.com/facebookresearch/fairseq2/blob/v0.6.0/README.md)

```text
Python 3.11.14
torch==2.8.0+cpu
torchaudio==2.8.0+cpu
fairseq2[arrow]==0.6.0
fairseq2n==0.6+cpu
omnilingual-asr==0.2.0
numpy==1.26.4
pyarrow==20.0.0

PyTorch CPU index: https://download.pytorch.org/whl/cpu
fairseq2 CPU index: https://fair.pkg.atmeta.com/fairseq2/whl/pt2.8.0/cpu
```

The following **unexecuted adapter sketch** uses the official model/pipeline interfaces. `pcm16k_mono` is a real, unpadded 16 kHz mono window, and `normalized_qwen_text` must retain a reversible map to the original transcript. `sonefold_omni_ctc_v1_local` stands for a locally registered card pointing to already verified v1 weights and their paired local tokenizer, not a built-in name. The worker must separately bound input length, validate finite samples and reject empty targets.

```python
import torch
from fairseq2.nn.batch_layout import BatchLayout
from omnilingual_asr.models.inference.pipeline import ASRInferencePipeline

pipe = ASRInferencePipeline(
    model_card="sonefold_omni_ctc_v1_local",
    device="cpu",
    dtype=torch.float32,
)
waveform = torch.as_tensor(pcm16k_mono, dtype=torch.float32).reshape(1, -1)
waveform = torch.nn.functional.layer_norm(waveform, waveform.shape[-1:])
layout = BatchLayout(waveform.shape, seq_lens=[waveform.shape[1]], device=waveform.device)
with torch.inference_mode():
    logits, output_layout = pipe.model(waveform, layout)
    valid_frames = output_layout.seq_lens[0]
    log_probs = logits[0, :valid_frames].float().log_softmax(dim=-1)
targets = pipe.tokenizer.create_encoder()(normalized_qwen_text).to(torch.int64)
unk_id = pipe.tokenizer.vocab_info.unk_idx
if unk_id is not None and torch.any(targets == unk_id):
    raise ValueError("Unsupported target symbols; retain coarse timing")
if not targets.numel() or torch.any(targets == 0):
    raise ValueError("Empty target or CTC blank in target")
# CTC-align log_probs[T, V] to targets[U], with blank_id=0.
# Preserve original text; emit unknown/coarse timing when alignment fails.
```

The normalization mirrors Omni's zero-mean/unit-variance helper. fairseq2 v0.6.0's forward returns logits plus `BatchLayout`, and its CTC loss uses PyTorch's default blank index 0. These are source-supported interfaces, not a tested Sonefold implementation. Use a bounded local CTC implementation; TorchAudio 2.8's forced-align API is deprecated, as noted below. [Audio preprocessing](https://github.com/facebookresearch/omnilingual-asr/blob/main/src/omnilingual_asr/datasets/utils/audio.py), [versioned model forward](https://github.com/facebookresearch/fairseq2/blob/v0.6.0/src/fairseq2/models/wav2vec2/asr/model.py), [versioned layout](https://github.com/facebookresearch/fairseq2/blob/v0.6.0/src/fairseq2/nn/batch_layout.py)

**Implementation-session evidence supplied after this source review:** original v1 revision `8e35f0cc28fa6099e0c14d56db85ce0423baa691`, model SHA-256 `4d31c6a7725a03e90cf26080c67361928755a9f43d322dd574145fbe91c76aec`, paired-tokenizer SHA-256 `b954cc166b0c9e0271b953fa226fa27ca706a25b7029e84579fe2c60a2b451fe`. A 30-second CPU pass reportedly took 5.12 seconds plus 0.055 seconds for alignment. A local HF conversion matched all 423 tensors; on one identical normalized 30-second input, FP32 logits differed by maximum 0.000423 / mean 0.0000052 with identical argmax tokens (about 6.4 seconds versus 5.1 seconds official runtime). This is one-input parity evidence, not validation of arbitrary languages, lengths or deployment.

The same follow-up reported exact Qwen text preservation and reduced unknown-character attribution on normal Hindi (100%→1.8%), hard Hindi (69%→0.24%) and very-hard Vietnamese (100%→1.1%). These are **coverage diagnostics**, not correct-speaker accuracy: the normal Hindi example still had six reference voices versus four Nemotron clusters and about 20% attribution conflict. They support continuing the alignment work while measuring speaker errors independently.

TorchAudio's MMS tutorial requires language-dependent normalization, romanization and appropriate segmentation. It also warns that its forced-alignment APIs were deprecated in 2.8 and scheduled for removal in 2.9. Do not build a new unpinned integration around their continued availability. Keep a reversible mapping from normalized/romanized tokens to the original writing system; never replace the stored transcript with the aligner's normalization. [Official tutorial](https://docs.pytorch.org/audio/stable/tutorials/forced_alignment_for_multilingual_data_tutorial.html)

Alignment places supplied text against audio; it does not prove that supplied words are correct. Reject impossible/nonmonotone/out-of-bounds intervals and measure alignment coverage and boundary error independently of lexical fidelity.

## 2. Treat overlap as a separate acoustic problem

**Evidence:** NVIDIA's integration distinguishes an ASR model conditioned on speaker activity from an activity-masked ASR stream. These are model-specific integrations. They do not establish that multiplying Qwen's input by a diarization mask separates two voices that speak simultaneously. [NVIDIA ASR integration guide](https://huggingface.co/nvidia/Nemotron-3-Diarization/blob/main/ASR_INTEGRATION_GUIDE.md)

**Recommendation:** use ordinary Qwen decoding on clean single-speaker intervals; run any separation only on detected overlap plus context, as an optional second pass. Reassociate separated streams to surrounding clean speaker embeddings, testing both channel permutations. Score leakage, dropped quiet speakers, duplicated words and invented words. Keep the mixture and original transcript. A failed separation should remain reviewable overlap, not become confident attribution.

A concrete local, permissive research baseline is SpeechBrain's **SepFormer Libri2Mix** checkpoint, released 2022-09-16 under Apache-2.0. Its card documents **8 kHz single-channel input**, two output streams and source-separation metrics on Libri2Mix. Those metrics are not meeting WER or evidence of multilingual/three-person-overlap accuracy. Resampling and domain mismatch make it a conditional experiment, not the default fix. [Checkpoint card](https://huggingface.co/speechbrain/sepformer-libri2mix)

Guided Source Separation is another option when actual synchronized microphone channels exist; its documented workflow takes a multichannel recording plus speaker segments. The current mono mixture cannot recover those discarded spatial channels. Preserving separate microphone/system tracks, where available and consented, is a distinct capture improvement; a system-audio track may still contain several remote participants. [GSS implementation](https://github.com/desh2608/gss)

Community-1 supplies both regular and exclusive diarization, and supports speaker-count bounds. Exclusive turns help reconcile approximate text timing, but choosing a single label does not reconstruct the other simultaneous voice's words. It is a useful optional comparison for count/reconciliation failures, with CC-BY-4.0 weights and gated initial access. [Community-1 card](https://huggingface.co/pyannote/speaker-diarization-community-1)

## 3. Use bounded language and vocabulary context

Qwen's official inference wrapper accepts both `context` and `language`; a supplied language changes the generated prompt to force that language's transcription output. [Official Qwen inference source](https://github.com/QwenLM/Qwen3-ASR/blob/main/qwen_asr/inference/qwen3_asr.py)

**Recommendation:** compare automatic language identification with an explicit hint on confidently monolingual spans. Keep automatic/multilingual routing for code-switching; avoid forcing an entire English/Hindi meeting to one language. Build small context lists from accepted project vocabulary, actual participant names and confirmed acronyms, including relevant native spelling. Do not pass speculative knowledge or long previous summaries as acoustic truth. Measure entity recall **and false insertion rate** with correct, irrelevant and deliberately wrong hints. A vocabulary feature can increase plausible errors when hints are wrong; local evaluation must establish the tradeoff.

## 4. Stabilize identities using clean acoustic evidence

CAM++ was designed as an efficient speaker embedding network and evaluated on VoxCeleb/CN-Celeb verification. That supports retaining a lightweight embedding component, not a claim of calibrated identity certainty on every language or noisy meeting. The specific FunASR CAMPPlus checkpoint is tagged Apache-2.0. [CAM++ paper, 2023](https://arxiv.org/abs/2303.00332), [checkpoint](https://huggingface.co/funasr/campplus)

**Recommendations:**

- Extract several clean, non-overlapping examples per anonymous speaker; test a duration sweep rather than trusting one short utterance. Keep microphone/channel and quality metadata. Exclude separation artifacts, clipping and music from enrollment.
- Normalize embeddings, retain multiple exemplars, and compare a robust speaker centroid plus individual samples. Tune duration/quality rules on development meetings. Require consistent matches and a margin over the runner-up; otherwise abstain.
- Calibrate thresholds on **same-speaker and different-speaker trials from the intended languages/channels**, including unenrolled people. Report false accepts, false rejects and unknown rate. Cosine similarity is a score, not a percentage confidence; a library default threshold is not a calibrated policy.
- Before merging two anonymous clusters, require repeated acoustic agreement and no evidence that the two voices are concurrently active. Concurrent activity is a useful conflict signal, subject to diarization error. Avoid transitive single-link merges where A≈B and B≈C collapse distinct A/C speakers.
- Map named people only after explicit confirmed enrollment or user mapping. Keep project scope and biometric deletion semantics. Similar text, roles or turn-taking are insufficient identity evidence.

Domain, duration, noise and channel mismatch affect verification scores; published calibration/normalization research supports measuring this on matched local data. It does not provide a universal CAMPPlus threshold. [Cumani and Sarni, Interspeech 2023](https://www.isca-archive.org/interspeech_2023/cumani23_interspeech.pdf), [NeMo recognition concepts](https://docs.nvidia.com/nemo/speech/nightly/asr/speaker_recognition/intro.html)

### Follow-up shortlist: a second acoustic verifier

**Recommendation:** prototype **SpeechBrain `spkrec-ecapa-voxceleb`** as the first Apache-2.0 alternative. It provides a different checkpoint/training corpus and architecture from the existing Chinese CAMPPlus. It is an independent acoustic *check*, not statistically independent evidence or a guarantee of better meeting performance. Do not let agreement between two models count twice as ground truth.

| Official candidate | Weights, size and local execution | Decision for this project |
| --- | --- | --- |
| SpeechBrain ECAPA-TDNN, `speechbrain/spkrec-ecapa-voxceleb` | **Apache-2.0** model card; trained on VoxCeleb1+2. Embedding checkpoint **83.3 MB**, full repository 89.1 MB. Configuration produces **192-dimensional** vectors from 80-bin features. CPU inference is supported; use explicit local paths and the documented 16 kHz mono input. [Card](https://huggingface.co/speechbrain/spkrec-ecapa-voxceleb), [files](https://huggingface.co/speechbrain/spkrec-ecapa-voxceleb/tree/main), [configuration](https://huggingface.co/speechbrain/spkrec-ecapa-voxceleb/blob/main/hyperparams.yaml) | Best first experiment under the Apache preference. Extract embeddings through `EncoderClassifier.encode_batch`; match the checkpoint's feature normalization. The published 0.80% clean-VoxCeleb EER is a verification benchmark, not a meeting-speaker accuracy prediction. |
| WeSpeaker VoxCeleb ResNet34-LM | **CC-BY-4.0 weights**, Apache-2.0 toolkit. **6.63M** embedding parameters / **256 dimensions**; official ONNX **26.5 MB**, training checkpoint 45.1 MB. The documented local loader accepts CPU (`set_gpu(-1)`), and ONNX is a compact deployment route. [Card](https://huggingface.co/Wespeaker/wespeaker-voxceleb-resnet34-LM), [files](https://huggingface.co/Wespeaker/wespeaker-voxceleb-resnet34-LM/tree/main) | Strong compact architectural comparison if attribution-license weights are acceptable. It is not an Apache-only artifact. Large-margin tuning targets longer utterances; test short fragments separately. |
| WeSpeaker VoxCeleb ResNet221-LM | Official ONNX **95 MB**, checkpoint 114 MB. HF metadata says **Apache-2.0**, but the README body is empty and the toolkit's pretrained-model policy says VoxCeleb models follow **CC-BY-4.0**. [Files/card](https://huggingface.co/Wespeaker/wespeaker-voxceleb-resnet221-LM/tree/main), [project weight-license policy](https://github.com/wenet-e2e/wespeaker/blob/master/docs/pretrained.md) | Deeper, more expensive comparison after ResNet34. **Unresolved license conflict:** do not describe it as unambiguously Apache-2.0 or bundle it on that assumption. No representative CPU speed verified here. |
| Author-released ECAPA2, `Jenthe/ECAPA2` | **CC-BY-NC-4.0**, not Apache-2.0. The author documents CPU TorchScript loading and 16 kHz audio. Weight size was not verified in this review. [Author card](https://huggingface.co/Jenthe/ECAPA2) | Exclude from the default distributable fallback under the project's licensing preference. Its research merits do not remove the noncommercial condition. |

The WeSpeaker project documents both PyTorch and ONNX runtime models; its general weight-license policy is separate from its Apache toolkit license. The 2021 SpeechBrain release, WeSpeaker's published 2022/2023 work and ECAPA2's 2023/2024 release precede the cutoff. Pin model/configuration revisions and complete local loading before measuring throughput. No candidate above was downloaded or tested by this research task. [WeSpeaker pretrained documentation](https://github.com/wenet-e2e/wespeaker/blob/master/docs/pretrained.md), [SpeechBrain model history](https://huggingface.co/speechbrain/spkrec-ecapa-voxceleb/commits/main)

The follow-up CAMPPlus experiment reportedly collapsed an eight-voice case to two inferred clusters, achieved only about 84% best tuned fragment matching, and remained near 80% when given the oracle count. That suggests that count selection alone does not explain the failure. Keep oracle-count results labeled diagnostic; never use them as the product score.

**Evaluation design:** hold the same audio boundaries constant and compare CAMPPlus versus ECAPA on (a) same/different-speaker pairs and (b) full-meeting clustering. Use clean, non-overlapping excerpts first, then short/quiet/overlapped/degraded excerpts; compare several durations and multiple exemplars per person. If clean reference-speaker samples already overlap heavily in embedding space, inspect synthetic voice distinctness and channel effects before tuning clustering. Otherwise locate the fragment-quality or boundary failures. Calibrate on development meetings with unseen speakers in the test split.

Use the second model to flag questionable merges/splits and named-voice matches for review. Require support from clean exemplars and a margin over alternatives; do not overwrite uncertain turns merely because a roster contains a plausible name. Measure false merges, false splits, missed voices, per-speaker attribution and abstention, alongside CPU time/RSS. Fixing Nemotron's missed sixth voice requires discovering/reassigning acoustic clusters, not simply filling every formerly unknown word. None of these candidates establishes 99% across the difficulty groups.

## 5. Make Qwen3.5 4B summaries verifiable by construction

The upstream Qwen3.5-4B card supplies a permissive Apache-2.0 model and framework-specific prompting/sampling advice, but no 99% multilingual meeting-factuality guarantee for the local Q4 GGUF. Quantization and prompting changes need paired testing against the actual installed artifact. [Qwen3.5-4B card](https://huggingface.co/Qwen/Qwen3.5-4B)

**Recommended generation contract:**

1. Extract atomic records before writing prose: claim type, actor, action/object, quantity, deadline, polarity/modality, decision status, source segment IDs and minimal exact evidence spans. Missing actor/deadline stays `unknown`; a proposal remains a proposal. Preserve original language and source timestamps.
2. Resolve updates only with evidence: a later correction can supersede an earlier amount/date; a rejection must not become an accepted decision. Keep links to the rejected and final statements. Require coreference evidence before replacing a pronoun with a named person.
3. Validate source IDs and quote bytes/fingerprints deterministically. Then verify that the full claim follows from the cited context, including nearby denial/correction. An exact quote can exist while failing to support the claim.
4. Have a separate verification pass answer focused questions from source context, without seeing the draft's proposed answer when feasible. Check who, what, how much, when, negation and certainty; return `supported`, `contradicted` or `insufficient`. Use disagreement to request review or omit a claim, not to invent a compromise.
5. Render the prose from accepted records with visible source navigation. Revalidate **the rendered prose**, because fluent rewriting can change a number, actor or modal verb. Deterministically check entities, dates and quantities where possible.

Atomic evaluation has support in FActScore. QAFactEval shows that question-answering and entailment signals can be complementary, and that component choice matters. Chain-of-Verification tests independent verification questions, but its tasks/models differ from these meetings. Together they justify an experiment in source-based verification, not a guarantee that the same 4B model can reliably certify itself. [FActScore](https://aclanthology.org/2023.emnlp-main.741/), [QAFactEval, NAACL 2022](https://aclanthology.org/2022.naacl-main.187/), [Chain-of-Verification, 2023](https://arxiv.org/abs/2309.11495)

Dialogue evaluation specifically identifies subject/object understanding as a source of factual errors. Include adversarial but natural fixtures for reassigned actions, quoted proposals, sarcasm, negation, corrected dates and multilingual coreference. Do not transfer the paper's percentages to Qwen3.5. [Dialogue factual consistency, NAACL 2024](https://aclanthology.org/2024.naacl-long.338/)

Measure summary quality twice: once from the **audio-validated reference transcript** and once from ASR. Their difference identifies upstream propagation; neither should be hidden by rewriting references. The existing same-family 4B judge remains a diagnostic. Independently review a stratified sample and every critical-error class with competent language reviewers; record uncertain/unintelligible audio explicitly. A reference fact erased by audio damage is not an obligatory audio-grounded answer.

## 6. Run a small ablation before another full evaluation

Freeze the current installation/checkpoints, prompts, hashes, seeds, output IDs and evaluation policy. Make new run directories; keep all prior results and source references. First use a compact **development slice separate from the final scoring slice**, covering unsupported alignment languages, one supported-language control, code-switching, short backchannels, 2/4/8 speakers, quiet speech and natural argument overlap.

| Order | Change tested against the frozen baseline | Main acceptance evidence |
| --- | --- | --- |
| A | Speaker-guided retranscription only for mixed approximate-timing windows | Less unknown attribution and lower speaker-attributed error without increased lexical omissions/duplication; preserve uncertainty where warranted. |
| B | One permissively licensed fallback aligner per initially targeted language | Alignment coverage, true boundary error and attribution improve; original text survives normalization unchanged. |
| C | Correct/irrelevant/wrong vocabulary hints; auto versus per-span language hints | Better entity recovery without unacceptable false insertions or code-switch loss. |
| D | Clean-exemplar embedding calibration and constrained cluster merging | Lower false named matches/cluster splits with documented abstention and no new conflations. |
| E | Atomic extraction plus source verification, using the same Q4 4B artifact | Higher factual precision with maintained critical-fact recall, including the final rendered prose. |
| F | Overlap-only separation | Better overlap-region cpWER and attribution; no degradation on ordinary speech, acceptable added cost. |

After selecting settings on development data, rerun the complete 200-meeting evaluation **without retuning on its final scores**. Keep each component's metrics separate. Optional oracle diagnostics—reference turns with Qwen, reference words with predicted diarization, reference transcript with Qwen summaries—locate error sources but must be labeled oracle, not product performance.

## Local operation, costs and release constraints

These recommendations require no paid API by default. The costs are additional local inference, model storage, engineering and independent reference review. Measure **added audio processed, cold load, warm real-time factor, p95 finalization, RAM/VRAM and cancellation**; do not infer laptop throughput from vendor server benchmarks. Load stages sequentially and cache bounded intermediates. Restrict expensive retries to flagged regions; cap retries to avoid a hard recording consuming unbounded compute.

When the GPU also drives the desktop, run heavy inference under a GPU-sharing launcher ([GPU notes](../GPU_ACCELERATION.md#sharing-the-gpu-with-the-desktop)).

Pin every downloaded artifact/revision/hash and tokenizer; review **weights and code licenses separately**. Qwen ASR/aligner/4B and the cited CAMPPlus/Libri2Mix checkpoints are tagged Apache-2.0; Nemotron-3 is **OpenMDW-1.1**, Community-1 is **CC-BY-4.0**, stable-ts is **MIT**, and the CTC wrapper's default model is **noncommercial**. Preserve required notices and do not label the whole optional stack Apache-2.0. The prior [diarization review](2026-09-30-diarization.md) records the existing deployment choices and should remain intact.

Use predownloaded local paths, disabled telemetry and a network-blocked verification run. Keep audio, embeddings and transcripts out of logs/commits. Treat meeting text and vocabulary as untrusted data; the summary verifier must have no publication/tool authority merely because the transcript contains instructions. Retain typed service boundaries and reviewable proposals. Failed optional components must not silently upload audio or install remote code.

## Date and version trace

The repository's [existing model-revision evidence](stt-evidence/model-revisions.json) records Qwen ASR `7278e1e70fe206f11671096ffdd38061171dd6e5` (2026-01-30), Qwen aligner `c7cbfc2048c462b0d63a45797104fc9db3ad62b7` (2026-01-30), and Nemotron-3 `f667ed73aee57d40cc39428eb768b4fd87a0a29e` (2026-09-24). The prior [summary smoke evidence](2026-09-30-summary-evidence.json) records the Q4_K_M artifact revision and SHA-256. These are inherited provenance, not newly benchmarked models.

Additional cutoff checks: Qwen's ASR release is dated 2026-01-29 in its repository; its technical report is [arXiv:2601.21337](https://arxiv.org/abs/2601.21337). NVIDIA's [dated release announcement](https://huggingface.co/blog/nvidia/nemotron-diarization) is 2026-09-23. Community-1's benchmark is explicitly dated 2025-09. Stable-ts's archive date is 2026-05-30. The CTC aligner [history](https://github.com/MahmoudAshraf97/ctc-forced-aligner/commits/main) shows `264e7a1` on 2026-04-15. Qwen3.5-4B's [history](https://huggingface.co/Qwen/Qwen3.5-4B/commits/main) includes its initial 2026-02 release. SepFormer's cited checkpoint dates to 2022. The cited papers are 2021–2024. Pin candidate source and weight revisions before implementing; this note does not claim that every dependency's current `main` was frozen.

**Remaining uncertainty:** no representative new inference or human review was performed for this note. The proposed gains, per-language alignment quality, separation benefit, calibrated speaker thresholds, summary-verifier reliability and laptop cost remain unmeasured. There is currently no evidence here that the three 99% targets have been achieved in any difficulty group, much less all three.

## Follow-up: alternatives after the eight-to-five speaker collapse

**Implementation-session evidence, not a published benchmark:** the additional independent ECAPA experiment did not recover the eight voices. Re-thresholding cached Nemotron posteriors from 0.2 through 0.7 still found five voices in `very-hard-024`. Changing 0.5 to 0.2 reduced its diagnostic DER from approximately 0.391 to 0.376, but worsened `normal-030` from 0.249 to 0.298 and did not recover all six reference speakers there. These results do not justify promoting a lower global threshold. They supersede the earlier suggestion that trying ECAPA alone might resolve this case; they do not establish that every possible ECAPA training or segmentation scheme would fail.

**Finding as of 2026-10-15:** this focused review found no small, clearly Apache-2.0 complete diarizer with published evidence establishing a reliable improvement over Nemotron-3 for this project's multilingual two-to-eight-person meetings. The useful next candidates change either the embedding architecture or the activity-inference mechanism. Their evidence supports controlled comparisons, not a 99% claim.

| Candidate | Primary evidence, license and size | Practical decision |
| --- | --- | --- |
| **3D-Speaker ERes2NetV2**, `iic/speech_eres2netv2_sv_zh-cn_16k-common@v1.0.1` | Different 2-D residual/multi-scale architecture from CAMPPlus and ECAPA. Official loader names `pretrained_eres2netv2.ckpt`; release notes date the 200k-speaker checkpoint to August 2024. Architecture is **17.8M parameters**, 192-dimensional embeddings; about **71 MB of FP32 parameters**, an estimate rather than a verified artifact size. **3D-Speaker code is Apache-2.0**. Follow-up retrieval of the [original ModelScope API metadata](https://modelscope.cn/api/v1/models/iic/speech_eres2netv2_sv_zh-cn_16k-common) on 2026-10-15 returned `Data.License: Apache License 2.0`. Pin and checksum the exact `v1.0.1` checkpoint before use; the original JSON was retained locally. [Official loader](https://github.com/modelscope/3D-Speaker/blob/main/speakerlab/bin/infer_sv.py), [release notes](https://github.com/modelscope/3D-Speaker), [original checkpoint](https://modelscope.cn/models/iic/speech_eres2netv2_sv_zh-cn_16k-common), [architecture](https://github.com/modelscope/3D-Speaker/blob/main/speakerlab/models/eres2net/ERes2NetV2.py) | Best compact **embedding** comparison still worth considering. Run clean reference-speaker pair tests before full clustering; stop if it cannot distinguish the voices under clean boundaries. It does not itself discover turns or fix mixed-speaker windows. |
| **Tuya `nomo-pvad` v1.1** | Target-speaker activity model with ERes2NetV2-derived backbone, FiLM conditioning and causal convolutions. **18.16M parameters / 36.6 MB FP16**, plus the separate original ERes2NetV2 enrollment encoder. Code **and the released fine-tuned weights explicitly declare Apache-2.0**; the original enrollment-model metadata was subsequently verified as Apache-2.0 as noted above. Official usage supports CPU, local model directories and **160 ms** outputs. [Official repository](https://github.com/tuya/nomo-pvad), [explicit weights license](https://github.com/tuya/nomo-pvad/blob/master/weights/WEIGHTS_LICENSE.md) | A materially different **activity verifier**, conditional on a clean known-speaker exemplar. It cannot discover an unrepresented speaker. For a diagnostic, supply an independently identified clean excerpt of a missed voice and test whether its activity can be recovered elsewhere. The reference-assisted result is oracle evidence until the product can obtain that exemplar autonomously. |
| **Audio-WestlakeU LS-EEND** | Online attractor extraction with recurrent retention, rather than cached Sortformer outputs or external embedding clustering. Official eight-speaker configuration uses four 256-dimensional encoder layers and two decoder layers. Exact checkpoint parameter count/bytes were not verified. **Code is MIT**, not Apache; a separate explicit license grant for linked checkpoint weights was not found in the reviewed upstream material. [Official implementation](https://github.com/Audio-WestlakeU/FS-EEND/tree/main/LS-EEND), [code license](https://github.com/Audio-WestlakeU/FS-EEND/blob/main/LS-EEND/LICENSE), [configuration](https://github.com/Audio-WestlakeU/FS-EEND/blob/main/LS-EEND/conf/spk_onl_conformer_retention_enc_dec_nonautoreg_infer.yaml) | Best small-architecture **whole-diarizer research comparison**, subject to confirming checkpoint licensing. It is not yet a cleared Apache replacement. Keep the one-to-eight-speaker simulation checkpoint distinct from domain-adapted AMI/CALLHOME/DIHARD checkpoints. |

ERes2NetV2's June 2024 paper reports VoxCeleb1-O EER of 0.98% for three-second test crops and 1.48% for two-second crops, with full-duration enrollment. Its experiments use VoxCeleb2/3D-Speaker training; those numbers must not be attributed to the separate 200k-speaker checkpoint or interpreted as eight-speaker meeting accuracy. The official ONNX runtime reports ERes2NetV2 RTF 0.142 versus CAMPPlus 0.049 on three-second input on a Xeon Platinum 8163, including feature extraction. This suggests plausible CPU use at greater cost than CAMPPlus, not a measured speed on this laptop. [Paper](https://arxiv.org/html/2406.02167v1), [official ONNX runtime and benchmark](https://github.com/modelscope/3D-Speaker/blob/main/runtime/onnxruntime/README.md)

The existing 3D-Speaker **complete pipeline is not a new end-to-end model**: it combines VAD, segmentation, embeddings and clustering. Its optional overlap module is the separately licensed, initially gated **MIT** `pyannote/segmentation-3.0`, so toolkit licensing alone does not describe the assembled artifact. The pipeline reports substantial overlap-related gains on some corpora but regressions on two internal Chinese meetings, and warns about short audio and large speaker counts. Its reported CPU RTF 0.03 lacks enough hardware/configuration detail to transfer to this deployment. Replacing only CAMPPlus with ERes2NetV2 while keeping the same faulty windows remains a different experiment from adopting this pipeline. [Pipeline and limitations](https://github.com/modelscope/3D-Speaker/tree/main/egs/3dspeaker/speaker-diarization), [overlap-model card](https://huggingface.co/pyannote/segmentation-3.0)

For `nomo-pvad`, the official diagram recommends at least ten seconds for enrollment, and the authors acknowledge difficulty with short enrollment, far-field/channel mismatch and similar voices. The wrapper requires the separate original checkpoint at initialization even when an embedding is already provided. One conditioned pass per candidate speaker would multiply inference work; that cost estimate is an inference, not a published eight-speaker benchmark. Restrict any prototype to disputed regions, retain concurrent activity instead of forcing exclusive labels, and calibrate false accepts as well as missed speech. Its score is not a calibrated identity probability. No independent multilingual meeting-quality or target-PC throughput evidence was found. [Official model/usage](https://github.com/tuya/nomo-pvad)

For an enrolled project-person experiment, the precise first-party artifact is [`tuya/nomo-pvad/weights/nomo_pvad.pt`](https://github.com/tuya/nomo-pvad/blob/master/weights/nomo_pvad.pt), displayed as **34.9 MB** by GitHub; [`weights/WEIGHTS_LICENSE.md`](https://github.com/tuya/nomo-pvad/blob/master/weights/WEIGHTS_LICENSE.md) explicitly licenses these fine-tuned weights under Apache-2.0. The separate enrollment checkpoint is **`iic/speech_eres2netv2_sv_zh-cn_16k-common` / `pretrained_eres2netv2.ckpt`**, supplied through `eres2netv2_dir`. The wrapper accepts mono **16 kHz float32 PCM** (normalized waveform scale: its frontend multiplies by 32768), computes an L2-normalized **192-dimensional ERes2NetV2** vector, and scores **2560-sample/160 ms** chunks. Its default session repeatedly evaluates a **2.4-second sliding window**, so output stride is not a measured CPU latency. CPU PyTorch is explicitly supported; no first-party ONNX export/artifact was located. Local checkpoint paths are necessary to avoid its automatic ModelScope download fallback. It loads state dictionaries non-strictly, so an integration should explicitly reject unexplained missing/unexpected tensors. [Official wrapper](https://github.com/tuya/nomo-pvad/blob/master/nomo_pvad.py)

**Recommendation for named voice memory:** version the encoder/preprocessing/checkpoint identity with every stored embedding. Existing CAMPPlus vectors cannot be reinterpreted as ERes2NetV2 vectors, even if dimensions happened to agree; regenerate from consented retained enrollment audio or obtain fresh enrollment. Keep unknown-person discovery and enrolled-person recognition as separate evaluations. For known people, test correct enrollment, wrong enrollment, absent enrolled people and acoustically similar impostors. An enrolled-person activity detector may help verify known names while leaving the cold-start eight-speaker discovery failure unresolved.

LS-EEND's September 2025 paper reports **23.35% DER on simulated eight-speaker mixtures**, with 0.25-second collar/median filtering, and **20.76% on AMI Eval**, without collar/filtering. Its reported single-thread CPU RTF is **0.028 on an AMD EPYC 7742**. These establish a feasible alternative design, not superiority on the current fixtures. The implementation uses **8 kHz audio** and a nominal **100 ms output step**; its streaming script supports CPU, but currently defaults to an additional batch-comparison pass and permits non-strict state loading with warnings. A future prototype must validate exact tensors, omit comparison work from timed production inference, and use the checkpoint's frontend. [Versioned paper](https://arxiv.org/html/2410.06670v2), [official evaluation details](https://github.com/Audio-WestlakeU/FS-EEND/tree/main/LS-EEND), [inference source](https://github.com/Audio-WestlakeU/FS-EEND/blob/main/LS-EEND/streaming_infer_dia.py)

Other leads do not satisfy this review's replacement constraints (a compact, distributable, clearly licensed model):

- **NeMo TitaNet-Large** is a different compact encoder, approximately **23M parameters**, but its official weights are **CC-BY-4.0**. It remains an attribution-license comparison, not an Apache-only answer. [Official card](https://huggingface.co/nvidia/speakerverification_en_titanet_large)
- **NeMo MSDD telephonic v1.0.1** is a **102.62 MB compressed** multi-scale decoder package, updated April 2023. It requires initial clustering, whose failures affect its final output. The model is trained on telephony, its published evaluation uses oracle VAD, and **NGC terms govern the weights**. NeMo's Apache code license does not override that. [Official NVIDIA card](https://catalog.ngc.nvidia.com/orgs/nvidia/nemo/models/diar_msdd_telephonic)
- **DiariZen**, including efficient pruned WavLM variants, has **MIT code and CC-BY-NC-4.0 weights**. **SUPlime/SUPlime-L**, released by September 2026, publish **114M/349M** model sizes but likewise **CC-BY-NC-4.0 weights** with MIT package code. Their newer benchmark claims do not clear the project's distributable-model preference. [DiariZen license](https://github.com/BUTSpeechFIT/DiariZen#license), [SUPlime card](https://huggingface.co/rewayai/suplime), [dated package release](https://pypi.org/project/suplime/)
- **EEND-M2F** is a relevant efficient architecture in a January 2024 paper, but this review did not locate a first-party, clearly licensed ready-to-use checkpoint. A paper's strong DER is insufficient to recommend an installable model. [Original paper](https://arxiv.org/abs/2401.12600)

**Recommended next decision:** keep the threshold change unpromoted. If one more compact acoustic experiment is justified, evaluate ERes2NetV2 on clean same/different-speaker trials first; separately evaluate a conditioned activity model only where an independently established clean exemplar exists. An LS-EEND comparison would test a different whole-diarizer mechanism once weight licensing is resolved. Record speaker-count recovery, per-speaker recall/confusion, overlap DER, CPU/RAM and any oracle assistance. None of these experiments should alter the original transcripts, reference speaker identities or benchmark normalization. No model weights were fetched, no inference was performed, and no production code was changed by this follow-up review.

## Additional context-biasing research checked on 16 October

The following publications meet this review's **15 October 2026** cutoff.
They motivate experiments; they do not establish 99% performance here.

- **LLM-HB**, submitted 12 October, combines a Whisper-medium encoder, a learned
  language-specialized adapter and a LoRA-adapted Qwen3-4B. Its Mandarin/English
  ASRU2019-CS evaluation reports 5.85% mixed error with fifteen distractors per
  utterance. That is a trained architecture, not a prompt that can be pasted
  into the existing Qwen3-ASR worker. Its vocabulary setup and languages differ
  from these multilingual meetings. [Versioned paper](https://arxiv.org/html/2610.16579v1)
- **Contextual Biasing for LLM-Based ASR with Hotword Retrieval and Reinforcement
  Learning**, submitted December 2025, first retrieves a compact vocabulary set
  using audio/text representations, then trains contextual ASR with a task-driven
  reward. It supports testing selective, acoustically grounded vocabulary rather
  than assuming a large project roster always improves recognition. It does not
  validate the untrained CTC selection heuristic used in this repository's new
  diagnostic. [Versioned paper](https://arxiv.org/html/2512.21828v1)

Actual Overtalk trials and rejected alternatives were recorded in an internal
accuracy work log that is not published.


### Source-preserving summaries and independent checks (16 October follow-up)

- [ModernBERT-base NLI](https://huggingface.co/tasksource/ModernBERT-base-nli) is
  an Apache-2.0 English entailment classifier. Revision
  `de4ab7e77845098b7fab7f6ab9d370ddff27b19c` predates the research cutoff. Its
  small size makes local CPU calibration worth testing, but neither its softmax
  probabilities nor its general NLI scores establish 99% meeting factuality.
  The initial calibration tests ownership, deadlines, negation, quantities,
  approval conditions, and future commitments. It does not certify multilingual
  transcripts and is not installed in the app.
- [PrefixNLI, November 2025](https://arxiv.org/html/2511.01359v1) investigates
  factuality-guided decoding. This supports investigating independent entailment
  checks, not a claim that adding a generic verifier makes Qwen accurate. Its
  Llama-based model does not meet this project's Apache-2.0 model requirement.
- [NVIDIA TitaNet Large](https://huggingface.co/nvidia/speakerverification_en_titanet_large)
  is licensed CC-BY-4.0, despite the NeMo code being Apache-2.0. It was not
  downloaded or substituted for the project's permissively licensed pipeline.

A local replay showed a separate practical failure: the final report-writing
pass changed a pending review into apparent completed approval. The candidate
standard-template path renders the extracted notes directly and retains the
source references, removing that extra rewrite. This prevents that particular
mutation but cannot repair an incorrect extraction or incorrect ASR. Custom
context/templates and non-English translation require their own validation.


### Speaker uncertainty and model-license boundary

[WeSpeaker's U³-xi integration](https://github.com/wenet-e2e/wespeaker/pull/469),
merged 22 September 2026, propagates learned embedding covariance and adds
uncertainty-aware scoring. It needs a trained uncertainty-producing model; a
cosine threshold or crop variance is not an equivalent implementation. Its
current integration also lacks TorchScript/ONNX export. It is a research lead,
not a shipped low-resource fix.

The official [ResNet293-LM model card](https://huggingface.co/Wespeaker/wespeaker-voxceleb-resnet293-LM)
licenses its weights CC-BY-4.0. The Apache license on the surrounding toolkit does
not change that; no model substitution was made.
