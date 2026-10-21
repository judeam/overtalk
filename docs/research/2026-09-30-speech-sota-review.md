# Speech SOTA review for Sonefold — 2026-09-30

Research cutoff: **2026-09-30**. This review follows the original papers, model cards, checkpoint metadata, source code, and dataset owners. Published results below are author-reported unless a paper explicitly evaluates another system. No models, recordings, CUDA jobs, or installations were run for this review. No display or audio settings were changed.

This extends [the STT investigation](2026-09-30-stt.md) and [the diarization investigation](2026-09-30-diarization.md). Those reports already cover the original Qwen3-ASR package, native Parakeet, NeMo batch recognition, Nemotron streaming integration, Community-1, deployment troubleshooting, and initial smoke measurements. The repository observations here describe the worker and benchmark at the start of this review; implementation work performed alongside this research may address some of them.

## Recommended order of work

The largest remaining uncertainty is accuracy on real meetings, especially omitted speech, overlapping voices, and speaker changes. Synthetic English/German examples establish that the adapters communicate correctly. They do not establish meeting WER, speaker quality, or a latency advantage on this laptop.

| Priority | Concrete work | Decision it enables |
| --- | --- | --- |
| 1 | Add portable prediction exports, complete-meeting references, speaker-aware scoring, and recorded live event traces. Compare the native engines as well as optional workers. | Establish a real baseline and attribute errors before changing the default. |
| 2 | Add an isolated, optional **Cohere Transcribe** adapter using the original checkpoint and native Transformers implementation. Keep explicit language and timestamp capabilities truthful. | Compare a strong omitted offline model against Qwen on English/German meetings at the same normalization and segmentation. |
| 3 | Evaluate **MOSS Transcribe Diarize** through a separate reviewed runtime. | Test whether a compact joint model improves offline speaker-attributed transcription over ASR followed by diarization. |
| 4 | Sweep the already implemented **Nemotron-3.5/Nemotron-3** streaming profiles and the English speaker-conditioned path. Score with predicted and oracle diarization separately. | Choose a useful latency/accuracy operating point; find whether recognition or diarization causes the German deletions. |
| 5 | Trial **Parakeet Ultra** after checking Photon support on the RTX 5090 Laptop and runtime distribution terms. Treat Redux as an efficiency candidate. | Determine whether post-training improves the current model family without accepting an unsupported runtime or a quality regression. |
| 6 | Add alignment coverage, chunk-seam, silence, punctuation, language-switch, and raw/denoised ablations. | Improve the surrounding pipeline without attributing every failure to the ASR checkpoint. |

These are implementation priorities inferred from the repo and the evidence below, not a universal model ranking. Maintain separate choices for live recognition, offline cleanup, CPU fallback, and languages that the preferred engine does not support.

### Chosen primary profiles: strongest practical available options

Use the following as the primary comparison/default profiles for this Linux RTX 5090 Laptop project. Whisper and Community-1 are regression controls or fallbacks; they are not the preferred SOTA baseline. These selections reflect the best justified combination of capability, released weights, runtime, and size identified here. They do not invent local accuracy measurements.

| Workload | Selected primary profile | Evidence and practical reason |
| --- | --- | --- |
| Offline known English, single language | **Cohere Transcribe 2B** | Strong matched English/AMI author results, original Apache weights, native local Transformers runtime. |
| Offline known German and other supported single languages | **Cohere Transcribe 2B**, with original **Qwen3-ASR-1.7B** as the mandatory matched challenger | German is supported and the author's multilingual comparison is favorable. There is no independent German-meeting head-to-head here; Qwen retains the broader language/switching contract. |
| Offline automatic multilingual/switching and broader language coverage | **Original Qwen3-ASR-1.7B + original forced aligner where supported** | Existing working original adapter, 30-language ASR with automatic language identification, broader capabilities than the single-language Cohere contract. |
| Live multilingual, including German | **Nemotron-3.5-ASR-Streaming + Nemotron-3-Diarization** | Released cached streaming models, 32 ready locales and eight-speaker activity. Evaluate balanced and accuracy-focused context presets. |
| Live English with overlapping speakers | **Existing speaker-conditioned Multitalker RNNT + Nemotron-3-Diarization** | Prefer conditioned recognition over merely assigning labels to mixed-stream words; the controlled September study supports the architecture. Its trained SSA weights differ from this shipped path. |
| Offline joint speaker-attributed/overlapping meetings | **MOSS Transcribe Diarize 0.9B** | Original compact Apache checkpoint with multilingual joint output; compare against the strongest separate-stage profile rather than a Whisper/Community cascade. |
| Accuracy-oriented offline overlap research reference | **DiCoW**, with license-compatible diarization input for a shippable experiment | The September study independently reproduces strong offline cpWER. Default DiariZen's noncommercial weights need a different deployment decision. |

Sources and exact scope of each comparison follow below. The Cohere author also reports competitive multilingual evaluation, but that is still vendor evidence, not an independently controlled German meeting test. Source: [original multilingual release results](https://huggingface.co/blog/CohereLabs/cohere-transcribe-03-2026-release).

A SOTA baseline should include these strongest applicable profiles now, rather than waiting for every model to be measured before adding them. Promotion to an automatic user-facing choice should still require successful local operation, honest capability metadata, and the relevant held-out quality/latency checks. Ultra is an additional challenger pending hardware/runtime verification; Redux is a constrained-resource option.

## What the existing implementation already gets right

The inspected worker already includes original Qwen3-ASR 0.6B/1.7B with optional Qwen forced alignment; NeMo Parakeet; offline Nemotron and Community-1; and persistent cached speaker-tagged RNNT streams. Native Parakeet/Whisper remain separate paths. The earlier reports document fixes for resampling, silence/pause clocks, CUDA warmup, bounded ingress, worker cancellation, and cumulative segment snapshots. Recommending those same adapters as new work would duplicate completed work. Sources: [worker README](../../speech-worker/README.md), [ASR implementation](../../speech-worker/sonefold_speech/asr.py), [streaming implementation](../../speech-worker/sonefold_speech/streaming.py), and the two earlier reports.

The current text benchmark computes exact edit distance and records WER/CER, elapsed time, and empty-reference insertions. Its NFKC/casefold/punctuation normalization is useful as a local protocol, but differs from OpenASR. It has no S/D/I breakdown, speaker references, word timing evaluation, live event history, paired uncertainty estimate, or memory measurement. Its supported-model list covers the original worker engines rather than every native and candidate path. Source: [benchmark_asr.py](../../speech-worker/tools/benchmark_asr.py).

The inspected Qwen windowing uses up to 30 seconds and searches the last three seconds for a quiet boundary; otherwise it makes a hard cut. It has no context overlap or seam reconciliation. Only exact-zero audio is skipped. Word alignment preserves ASR presentation text where supported, with explicit chunk timestamps otherwise. Speaker assignment uses temporal support thresholds, and its `speaker_confidence` is coverage evidence rather than a calibrated probability. Preserve those distinctions in export and evaluation. Sources: [asr.py](../../speech-worker/sonefold_speech/asr.py), [diarization.py](../../speech-worker/sonefold_speech/diarization.py).

## Strong candidates missing from the earlier comparison

### Cohere Transcribe: straightforward offline comparison

Cohere released its 2B Conformer encoder/decoder model on **2026-03-26**. It supports 14 languages including English and German and is Apache-2.0. On the release's eight-dataset English protocol it reports mean WER **5.42%**, versus **5.76%** for Qwen3-ASR-1.7B; AMI is **8.15%** versus **10.56%**. This is a useful within-table comparison, not proof of superiority on German, live meetings, or Sonefold's capture mix. The model-card machine-readable AMI result is 8.13%; retain the specific table/version rather than silently treating both as identical. Sources: [Cohere release article](https://huggingface.co/blog/CohereLabs/cohere-transcribe-03-2026-release), [original model card](https://huggingface.co/CohereLabs/cohere-transcribe-03-2026).

Its API requires a supported language rather than providing the Qwen-style automatic language contract. The card acknowledges inconsistent language switching and eager transcription of quiet/noisy input. It supplies text and optional punctuation, without native speaker labels or word timestamps. Therefore the first comparison should use an explicit language, bounded local windows, and an external aligner where available. Do not advertise automatic switching or precise timings on that adapter. Source: [Cohere model limitations](https://huggingface.co/CohereLabs/cohere-transcribe-03-2026).

The observed original checkpoint revision is `b1eacc2686a3d08ceaae5f24a88b1d519620bc09`, last modified **2026-06-10**. Access is automatically gated behind the owner's contact-sharing conditions even though the weights have an Apache license. Unauthenticated commit history returned 401, so this review verifies the public repository metadata and runtime source, not every weight-history entry. Source: [public model metadata](https://huggingface.co/api/models/CohereLabs/cohere-transcribe-03-2026).

Use a **separate environment**. Native `CohereAsrForConditionalGeneration`, `AutoModelForSpeechSeq2Seq`, and the processor are available in Transformers **5.4.0**; the author's example uses PyTorch 2.10. The original `qwen-asr==0.0.6` environment pins Transformers 4.57.6 and should not be upgraded in place. Sources: [Cohere setup example](https://huggingface.co/blog/CohereLabs/cohere-transcribe-03-2026-release), [native model implementation at v5.4.0](https://github.com/huggingface/transformers/blob/v5.4.0/src/transformers/models/cohere_asr/modeling_cohere_asr.py), [Qwen package dependencies](https://github.com/QwenLM/Qwen3-ASR/blob/main/pyproject.toml).

The native feature extractor expects **16 kHz** audio. Its default maximum chunk is **35 seconds**; audio up to 30 seconds takes the fast path. The five-second `overlap_chunk_second` parameter is a backward low-energy boundary search, explicitly **not overlapping audio context**. Longer input becomes a batch of chunks with `audio_chunk_index` bookkeeping. Sending a whole hour in one call can consequently create a large batch; use bounded windows/batches for a single laptop session. Source: [v5.4.0 feature extractor](https://github.com/huggingface/transformers/blob/v5.4.0/src/transformers/models/cohere_asr/feature_extraction_cohere_asr.py).

The following illustrates the reviewed contract, not an executed GPU test. `local_checkpoint` is the downloaded original revision, and `mono_16k` is a finite, resampled float32 array. Keep integer IDs/masks integer when moving inputs to the device.

```python
import torch
from transformers import AutoProcessor, AutoModelForSpeechSeq2Seq

processor = AutoProcessor.from_pretrained(
    local_checkpoint, local_files_only=True, trust_remote_code=False
)
model = AutoModelForSpeechSeq2Seq.from_pretrained(
    local_checkpoint, local_files_only=True, trust_remote_code=False,
    dtype=torch.bfloat16,
).to(device).eval()
inputs = processor(
    mono_16k, sampling_rate=16000, language="de",
    punctuation=True, return_tensors="pt",
)
chunk_index = inputs.pop("audio_chunk_index", None)
inputs = {key: value.to(device) for key, value in inputs.items()}
inputs["input_features"] = inputs["input_features"].to(model.dtype)
with torch.inference_mode():
    output_ids = model.generate(**inputs, do_sample=False, max_new_tokens=1024)
texts = processor.decode(
    output_ids, skip_special_tokens=True,
    audio_chunk_index=chunk_index, language="de",
)
```

The processor builds `decoder_input_ids` from the required language and punctuation flag. It returns features, a mask, and chunk metadata; decoding can reassemble chunk outputs into one string per original input. A generation budget such as 1024 needs measured validation and explicit detection of token-limit truncation; blindly copying the short-demo limit of 256 can lose dense speech. Sources: [v5.4.0 processor](https://github.com/huggingface/transformers/blob/v5.4.0/src/transformers/models/cohere_asr/processing_cohere_asr.py), [generation implementation](https://github.com/huggingface/transformers/blob/v5.4.0/src/transformers/models/cohere_asr/modeling_cohere_asr.py).

### MOSS Transcribe Diarize: compact joint meeting transcription

The original **0.9B MOSS Transcribe Diarize** checkpoint was released **2026-07-09**, under Apache-2.0. Its card supports 50+ languages, long audio up to 90 minutes, speaker segments, timestamp output, and contextual prompts. The observed revision is `704aa4a9c304e8520be88901e0d1960158ef5b15` (**2026-09-02**). This is a materially different offline candidate from another ASR-plus-clustering combination. The long-audio claim is not a guarantee of memory use on this laptop. Sources: [original card](https://huggingface.co/OpenMOSS-Team/MOSS-Transcribe-Diarize), [checkpoint history](https://huggingface.co/api/models/OpenMOSS-Team/MOSS-Transcribe-Diarize/commits/main).

Its July leaderboard submission reports seven-dataset mean WER **5.17%**, AMI **8.40%**, and Earnings22 **9.01%**. It omits TED-LIUM, so its mean cannot be ranked against Cohere's eight-dataset 5.42%. The card's AISHELL-4/AliMeeting speaker-attributed Chinese results are **cpCER**, not DER. Text recognition, speaker attribution, and time segmentation remain different quantities. Sources: [pinned submitted evaluation results](https://huggingface.co/OpenMOSS-Team/MOSS-Transcribe-Diarize/resolve/704aa4a9c304e8520be88901e0d1960158ef5b15/.eval_results/open_asr_leaderboard.yaml), [MOSS card's evaluation section](https://huggingface.co/OpenMOSS-Team/MOSS-Transcribe-Diarize).

The source at `61bc29cd4120be7b5d3b761b64cd5dff57263642` requires Transformers **>=5.6,<6** and lists PyTorch >=2.8 as a runtime dependency. It uses a custom `AutoModelForCausalLM` plus processor. Review and pin that package/code in a separate environment; `trust_remote_code=True` is executable custom code, not just checkpoint data. No recording URL or remote inference is necessary. Source: [pinned pyproject](https://github.com/OpenMOSS/MOSS-Transcribe-Diarize/blob/61bc29cd4120be7b5d3b761b64cd5dff57263642/pyproject.toml).

The package provides `build_transcription_messages(local_audio)`, `generate_transcription(...)`, and `parse_transcript(text)`. Generation returns a dictionary with `text`, `prompt_len`, and `generated_tokens`; pass its `text` to the parser. Parsed records contain `start`, `end`, `speaker`, and `text`; the format is `[start][Sxx]text[end]`. These are **segment timestamps**, not an aligned time for every word. The incremental text parser does not establish causal streaming ASR: it parses generation after audio has been supplied. Sources: [inference helpers](https://github.com/OpenMOSS/MOSS-Transcribe-Diarize/blob/61bc29cd4120be7b5d3b761b64cd5dff57263642/moss_transcribe_diarize/inference_utils.py), [transcript parser](https://github.com/OpenMOSS/MOSS-Transcribe-Diarize/blob/61bc29cd4120be7b5d3b761b64cd5dff57263642/moss_transcribe_diarize/transcript_parser.py).

```python
import torch
from transformers import AutoModelForCausalLM, AutoProcessor
from moss_transcribe_diarize.inference_utils import (
    build_transcription_messages, generate_transcription,
)
from moss_transcribe_diarize import parse_transcript

# local_checkpoint and installed source are reviewed and pinned revisions.
processor = AutoProcessor.from_pretrained(
    local_checkpoint, local_files_only=True, trust_remote_code=True,
)
model = AutoModelForCausalLM.from_pretrained(
    local_checkpoint, local_files_only=True, trust_remote_code=True,
    dtype=torch.bfloat16,
).to(device).eval()
result = generate_transcription(
    model, processor, build_transcription_messages(local_audio_path),
    max_new_tokens=4096, do_sample=False,
    device=torch.device(device), dtype=model.dtype,
)
segments = parse_transcript(result["text"])
```

This example uses a proposed bounded output budget, not a verified limit for arbitrary meeting length. Validate the path before passing it to the helper, which also accepts remote audio. The helper obtains audio rate from the feature extractor and prepares `input_ids`, `attention_mask`, `input_features`, `audio_feature_lengths`, and `audio_chunk_mapping`. The inference helpers are imported from their module, not the package root. Sources: [helper contract](https://github.com/OpenMOSS/MOSS-Transcribe-Diarize/blob/61bc29cd4120be7b5d3b761b64cd5dff57263642/moss_transcribe_diarize/inference_utils.py), [package exports](https://github.com/OpenMOSS/MOSS-Transcribe-Diarize/blob/61bc29cd4120be7b5d3b761b64cd5dff57263642/moss_transcribe_diarize/__init__.py).

A candidate adapter should retain raw generation for diagnosis, validate finite/in-range segment times, report skipped or truncated malformed output, and mark the timing source as joint segment generation. Speaker numbers are local anonymous labels; chunking requires measured identity reconciliation. Start with short complete meetings, then increase context and generation budgets. The helper's 131072 context limit and a short-demo output limit do not guarantee that a long meeting will finish without truncation. These are engineering recommendations from the source contract, not claimed defects demonstrated on recordings.

### Parakeet Ultra and Redux: new variants, new runtime

The **2026-09-23** Ultra checkpoint retains the 0.6B Parakeet-TDT architecture and 25-language tokenizer, with further training and a VAD head. Its original revision is `73175eb7aeb0d82f1e2a6b53b3aabc10a90bcd0b`, CC-BY-4.0. In the author's matched comparison, seven English datasets average **6.26% → 5.80%** WER, AMI **10.86% → 9.77%**, German FLEURS **4.13% → 3.61%**, and the 25-language FLEURS average **11.62% → 9.55%**. These aggregate improvements do not mean every language improves, and read-speech FLEURS is not a German meeting benchmark. Sources: [Ultra card](https://huggingface.co/moondream/parakeet-ultra), [original checkpoint metadata](https://huggingface.co/api/models/moondream/parakeet-ultra).

**Redux** is a smaller ternary encoder variant: 178 MB reported weights, revision `2bf128600aac4b16946f7ed8372e56117fe5e23b`, also CC-BY-4.0. The same card reports seven-dataset mean **6.55%**, versus original Parakeet 6.26%, and German FLEURS **5.42%**, versus 4.13%. It is an efficiency tradeoff rather than an accuracy upgrade. EPYC/M2 measurements do not establish speed on this laptop. Sources: [Redux card](https://huggingface.co/moondream/parakeet-redux), [Redux metadata](https://huggingface.co/api/models/moondream/parakeet-redux).

The documented Python path is `moondream.photon("moondream/parakeet-ultra").transcribe(audio=local_path, timestamps="word")`, returning segments with `word`, `start`, and `end`. Photon uses compiled kernels and publishes a hardware matrix; RTX 5090 Laptop support was not verified here. Neither model is a drop-in replacement for Sonefold's current ONNX files. Check the installed Photon version, chip support, and runtime redistribution terms independently of the weight license. Large-batch B200 throughput does not predict one live meeting's latency. Sources: [Ultra inference example](https://huggingface.co/moondream/parakeet-ultra), [Photon runtime documentation](https://moondream.ai/photon).

## Streaming and speaker attribution: evaluate the architecture

The original **Nemotron-3.5-ASR-Streaming-0.6B**, released 2026-06-04, supports 32 ready-to-use locales with automatic language tagging. Eight additional adaptation locales are not equivalent to ready models. Context configurations expose chunk durations from 80 to 1120 ms. The observed revision is `ea30d66debe3740a08b573244286791d423d6b3e` (2026-09-10), OpenMDW-1.1. Its current card distinguishes NeMo from Transformers >=5.13.0 RNNT support. Keep the implemented NeMo cache path pinned instead of assuming the older Qwen environment can load it. Sources: [pinned card](https://huggingface.co/nvidia/nemotron-3.5-asr-streaming-0.6b/blob/ea30d66debe3740a08b573244286791d423d6b3e/README.md), [metadata](https://huggingface.co/api/models/nvidia/nemotron-3.5-asr-streaming-0.6b).

**Nemotron-3-Diarization**, released 2026-09-23, supports eight simultaneous speaker channels and streaming cache configurations. The observed revision is `f667ed73aee57d40cc39428eb768b4fd87a0a29e` (2026-09-24), OpenMDW-1.1. Its DIHARD-III full-set results are DER **12.73%** with 30.4-second latency and **13.18%** at 1.04 seconds; AMI and several other evaluations use aligned references. These figures do not compare directly with a different collar, channel, scoring region, or pyannote corpus split. Source: [pinned original diarization card](https://huggingface.co/nvidia/Nemotron-3-Diarization/blob/f667ed73aee57d40cc39428eb768b4fd87a0a29e/README.md).

The integration guide provides masked-feature and speaker-conditioned RNNT paths. The earlier report already documents the corrected masking behavior and persistent state integration. The inspected worker uses a fixed roughly one-second ASR operating point with a specific diarization cache; the remaining task is quality and latency evaluation, including startup, cache growth, and speaker permutation stability. Sources: [original ASR integration guide](https://huggingface.co/nvidia/Nemotron-3-Diarization/blob/f667ed73aee57d40cc39428eb768b4fd87a0a29e/ASR_INTEGRATION_GUIDE.md), [worker streaming implementation](../../speech-worker/sonefold_speech/streaming.py).

An especially useful controlled study appeared **2026-09-09**. It compares a conventional cascade, masked ASR, word-level serialized output, and speaker-conditioned ASR at nominal **1.04-second** latency, using Nemotron English ASR and **older Sortformer-v2.1**, not the new September diarizer. Across CH109/Mixer6/AMI-IHM/AMI-SDM, real-diarization average cpWER is **42.27 / 34.77 / 33.46 / 23.36%**, respectively. On AMI-SDM, conditioned recognition scores **39.91%** with predicted diarization and **20.27%** with oracle diarization. Its reproduced offline DiCoW-v3.2 average is 15.11%, under a different latency regime. This motivates speaker conditioning and stage-specific oracles, rather than another claim that better labels alone recover missing words. PI-DTW is a training method for speaker permutation handling, not a post-processing aligner that can reconstruct omitted speech. Source: [NVIDIA authors' systematic architecture study](https://arxiv.org/html/2609.10265v1).

Community-1 remains a useful complementary offline control. It supplies regular overlap-aware and exclusive diarization; retain the regular timeline for DER and use exclusive turns only for display/text assignment. Its September 2025 zero-collar, overlap-included AMI-SDM result is **19.9%**, and DIHARD-III **20.2%**. Precision-2's stronger card scores use a different model and cloud/commercial access. Community-1 has gated CC-BY-4.0 weights. Source: [original pyannote card and benchmark protocol](https://huggingface.co/pyannote/speaker-diarization-community-1).

Speaker-conditioned **DiCoW** is a meaningful offline research reference. However, its default DiariZen checkpoint is **CC-BY-NC-4.0**, while the DiCoW source and weights have different licenses. Do not treat the default combined pipeline as an unrestricted commercial dependency. A measured experiment with another compatible diarizer is a separate integration task. Sources: [DiCoW source](https://github.com/BUTSpeechFIT/DiCoW), [original DiariZen checkpoint](https://huggingface.co/BUT-FIT/diarizen-wavlm-large-s80-md).

For endpointing, NVIDIA's English **Parakeet Realtime EOU 120M** exposes an end-of-utterance token and small cached RNNT steps. Its card's 160 ms median/320 ms p95 endpoint delay comes from TTS-generated DialogStudio with appended silence, not real meeting turns. It lacks punctuation/casing and is a specialized English candidate rather than a replacement for multilingual meeting text. Source: [original EOU card](https://huggingface.co/nvidia/parakeet_realtime_eou_120m-v1).

### Overlap is a recognition problem as well as a labeling problem

Adding two speaker labels to a single-speaker transcript cannot recover the quieter speaker's deleted words. Keep overlap-aware references and score overlapping words separately. Evaluate three concrete approaches: the current mixed-audio path; speaker-conditioned recognition; and joint MOSS transcription. An optional fourth path is ASR on independently retained microphone and system tracks with clock alignment and echo deduplication, if the captured signals contain separable speech. This is a proposed experiment, not a claim about the currently stored recording format.

Guided source separation can help with **synchronized physical multichannel arrays**, using diarization, spatial statistics, dereverberation, and beamforming. A microphone track plus loopback is not such an array; a mono mix cannot provide spatial information lost during mixing. Source: [original GSS implementation](https://github.com/desh2608/gss).

DOVER-Lap offers overlap-aware RTTM ensemble fusion, but its authors recommend at least three systems and caution that a two-system equal vote may degrade results. Do not assume that combining Nemotron and pyannote automatically improves DER. Source: [DOVER-Lap implementation and voting guidance](https://github.com/desh2608/dover-lap).

## Alignment, VAD, enhancement, punctuation, and switching

### Alignment must retain uncertainty and original text

Qwen's original forced aligner supports **11 languages** and inputs up to five minutes. The current bounded window design is inside that duration. Its language set is narrower than ASR, including Chinese/Cantonese, English, French, German, Italian, Japanese, Korean, Portuguese, Russian, and Spanish. Cohere-supported Arabic, Greek, Dutch, Polish, and Vietnamese therefore do not gain Qwen alignment merely by adding the Cohere adapter. The aligner can align external text, but cross-engine spelling and number presentation need explicit testing. Source: [original Qwen aligner card](https://huggingface.co/Qwen/Qwen3-ForcedAligner-0.6B).

WhisperX uses language-specific acoustic alignment and documents limitations for unsupported dictionary characters and overlap. It is useful as an optional comparison, not a guarantee that every number/currency token will acquire a time. NeMo Forced Aligner uses **CTC-based models**; it is not a generic timestamp switch for TDT/RNNT. Sources: [WhisperX alignment documentation](https://github.com/m-bain/whisperX), [NFA documentation](https://github.com/NVIDIA-NeMo/Speech/tree/main/tools/nemo_forced_aligner).

TorchAudio's `forced_align` API and `Wav2Vec2FABundle` were deprecated in 2.8 and removed in **2.9**. A tutorial using them is not an implementation plan for a freshly upgraded environment. Pin a working independent runtime or choose maintained alignment code. Source: [official TorchAudio migration notice](https://docs.pytorch.org/audio/stable/tutorials/ctc_forced_alignment_api_tutorial.html).

Export `timing_source` as `model_word`, `forced_alignment`, `joint_segment`, `chunk`, or `unavailable`. Keep raw recognition text, punctuation, and normalized scoring text separately. Measure boundary error on matched words **and alignment coverage**; a system that times only its easiest words must not appear better by excluding the difficult ones. Never distribute a segment's duration evenly across words and label those values as measured word timestamps.

### Better VAD is not a substitute for end-to-end tuning

The native continuous path uses Silero thresholds, minimum speech duration, and pre/post-roll; the NeMo live path preserves the complete audio timeline rather than removing silence before diarization. Keep that distinction. Tune speech recall and padding on development recordings with backchannels, quiet voices, laughter, music, floor noise, and short German phrases. Count deleted speech and false text after recognition, not only frame-level VAD F1. Source: [native VAD implementation](../../frontend/src-tauri/src/audio/vad.rs), [worker streaming implementation](../../speech-worker/sonefold_speech/streaming.py).

**FireRedASR2S**, released in February/March 2026, adds Apache-2.0 VAD/LID/punctuation components as well as Chinese/English ASR. FireRedVAD covers multilingual speech, singing, and music in streaming/offline modes; FireRedLID supports 100+ languages plus Chinese dialects. Its manufacturer metrics are not a measured win over Sonefold's Silero on meetings. FireRedPunc targets Chinese/English, so it is not a German punctuation solution. Sources: [original FireRed release chronology and component documentation](https://github.com/FireRedTeam/FireRedASR2S), [VAD card](https://huggingface.co/FireRedTeam/FireRedVAD), [LID card](https://huggingface.co/FireRedTeam/FireRedLID), [punctuation card](https://huggingface.co/FireRedTeam/FireRedPunc).

The observed component revisions are FireRedVAD `7990aaccc6b7aec1e527743bd30201f2c4a03b8c`, FireRedLID `1bb4d285c8456429385d9c0810300df4297bc11b`, and FireRedPunc `e448fd967f44182a1c323cc30f5d89f2400c28da`, all last modified **2026-03-13**. Sources: [VAD metadata](https://huggingface.co/api/models/FireRedTeam/FireRedVAD), [LID metadata](https://huggingface.co/api/models/FireRedTeam/FireRedLID), [punctuation metadata](https://huggingface.co/api/models/FireRedTeam/FireRedPunc).

Keep the existing MIT Silero as a baseline. **TEN-VAD is not plain Apache-2.0**: its actual license includes additional Agora noncompetition, deployment, and derivative-work conditions. Record those terms rather than copying the short badge into an unrestricted dependency list. Sources: [Silero repository](https://github.com/snakers4/silero-vad), [TEN-VAD license text](https://github.com/TEN-framework/ten-vad/blob/main/LICENSE).

### Enhancement should be optional and evaluated against raw capture

RNNoise and DeepFilterNet are practical CPU candidates. RNNoise operates on 48 kHz frames; DeepFilterNet provides real-time full-band enhancement under MIT/Apache source licensing. Neither is a new 2026 ASR checkpoint, and perceptual speech quality does not establish lower WER/DER. Sources: [RNNoise source](https://github.com/xiph/rnnoise), [DeepFilterNet source](https://github.com/Rikorose/DeepFilterNet).

Run identical raw/enhanced recordings through recognition, alignment, and diarization. Account for filter delay and final-frame padding, and inspect quieter overlapping voices. Store the original capture so denoising can be re-evaluated. The inspected RNNoise wrapper buffers complete 480-sample frames and flushes a partial tail; do not assume every call returns the same number of samples. This is an application-level transform; it does not require changing the system audio server's settings. Source: [native audio implementation](../../frontend/src-tauri/src/audio).

### Presentation and language changes need independent evaluation

Lexical WER should not hide punctuation/casing failures or number-format errors. Evaluate punctuation/case separately on manually reviewed references, plus names, amounts, dates, abbreviations, and negation recall. Keep verbatim and inverse-text-normalized references as explicitly different protocols. A generative punctuation pass can change lexical content; compare its word preservation and uncertainty rather than accepting plausible prose.

For English/German switches, compare automatic Qwen/Nemotron behavior with known-language and externally predicted-language conditions. Record language spans and mistakes, not only one recording-level code. A language identifier can provide segment-level routing, but a mistaken router can suppress a second language or add switching delay. Prompts/hotwords should be versioned and evaluated with both real terms and absent decoy terms to detect insertion bias. The original Qwen package accepts context; the inspected worker does not expose it. Sources: [Qwen API examples](https://github.com/QwenLM/Qwen3-ASR), [Nemotron original language contract](https://huggingface.co/nvidia/nemotron-3.5-asr-streaming-0.6b).

The original Fun-ASR-Nano standard checkpoint targets Chinese/English/Japanese; the separate MLT checkpoint's listed 31-language set does **not include German**. Neither card establishes native diarization/timestamps merely through repository tags. Original cards say Apache-2.0, while some converted variants have a different FunASR model license; verify the exact artifact rather than inheriting a family's badge. Sources: [standard original card](https://huggingface.co/FunAudioLLM/Fun-ASR-Nano-2512), [MLT original card](https://huggingface.co/FunAudioLLM/Fun-ASR-MLT-Nano-2512), [GGUF variant's license](https://huggingface.co/FunAudioLLM/Fun-ASR-Nano-2512-GGUF).

VibeVoice-ASR remains a larger offline joint-model reference: 9B, 50+ languages, long context, MIT weights. Its weight footprint alone makes it a lower-priority laptop comparison than 0.9B MOSS. Omnilingual ASR covers 1600+ language/script sets through a distinct fairseq2 runtime; it is valuable for language coverage, not evidence that it leads English/German meeting recognition. Sources: [VibeVoice original card](https://huggingface.co/microsoft/VibeVoice-ASR), [Meta's original Omnilingual source](https://github.com/facebookresearch/omnilingual-asr). The earlier STT report already covers Canary, Voxtral, Moonshine, Whisper, and Granite alternatives; their existence does not change the immediate evaluator gap.

## A reproducible evaluator for this repository

### Fix the public protocol before comparing headline WER

OpenASR is not one unchanging dataset. At source revision `7ce09ca6c5fb4aac32b4a528d906356a21ad5399` (**2026-09-25**), its dataset loader includes cleaned AMI/GigaSpeech/VoxPopuli families, ArtificialAnalysis-cleaned Earnings22 and VoiceArena, alongside legacy datasets. Its normalization/scoring uses language-specific normalizers and `kaldialign` compound merging. Sonefold's existing normalization should remain identified as its own protocol unless the exact leaderboard code, references, revision, and dataset composition are reproduced. Sources: [pinned dataset loader](https://github.com/huggingface/open_asr_leaderboard/blob/7ce09ca6c5fb4aac32b4a528d906356a21ad5399/normalizer/data_utils.py), [pinned scoring/normalization](https://github.com/huggingface/open_asr_leaderboard/blob/7ce09ca6c5fb4aac32b4a528d906356a21ad5399/normalizer/eval_utils.py).

Do not combine Cohere's March eight-dataset average, MOSS's July seven-dataset average, Ultra's seven-dataset comparison, and September cleaned references into one ordered table. Record per-dataset results, channel and segmentation, micro/macro aggregation, normalizer revision, and elapsed-time definition. A convenient cropped-utterance AMI WER and full-meeting speaker-aware AMI score answer different questions.

### Public audio that can provide real evidence

| Set and pinned source | Use | Provenance and limitations |
| --- | --- | --- |
| [`hf-audio/open-asr-leaderboard`](https://huggingface.co/datasets/hf-audio/open-asr-leaderboard), `b6bdcd0beb34f8975dc659796176d88f43aff502` (2026-06-26); select `ami` or `ami_cleaned` explicitly | Reproduce an identified ASR screening protocol | Repository-wide license metadata is absent; use each original corpus's terms. This does not supply a complete meeting speaker timeline by itself. [Dataset metadata](https://huggingface.co/api/datasets/hf-audio/open-asr-leaderboard). |
| [`edinburghcstr/ami`](https://huggingface.co/datasets/edinburghcstr/ami), `46f28f2503e2ec48f8867a84eef356c70476beab` (2026-01-13), `ihm`/`sdm`, `test` | Convenient genuine meeting utterances for WER and short-speech behavior | CC-BY-4.0, with meeting/speaker IDs and begin/end fields, but records are utterance crops. Concatenating them discards natural silence/overlap. [Metadata](https://huggingface.co/api/datasets/edinburghcstr/ami). |
| [Original AMI audio and annotations](https://groups.inf.ed.ac.uk/ami/download/) | Primary complete-meeting live replay, DER and cpWER/tcpWER | Corpus and annotations explicitly CC-BY-4.0. Freeze the [split](https://groups.inf.ed.ac.uk/ami/corpus/datasets.shtml), recording channel, annotation version, and scoring regions. [Original license](https://groups.inf.ed.ac.uk/ami/corpus/license.shtml). |
| [Microsoft NOTSOFAR](https://github.com/microsoft/NOTSOFAR1-Challenge), HF `microsoft/NOTSOFAR` revision `ba8fd0f034ce185fe4d24f47e53b4b8194795f07` (2026-06-21) | Real distant, overlapping meetings with ground truth | Owner states CC-BY-4.0. `240629.1_eval_small_with_GT` has 80 meetings, two devices per single/multichannel track, 16 hours per track. Subselect complete sessions/devices explicitly. Dev1 has speakers overlapping training. [Original data/download instructions](https://github.com/microsoft/NOTSOFAR1-Challenge#notsofar-1-datasets---download-instructions), [metadata](https://huggingface.co/api/datasets/microsoft/NOTSOFAR). |
| [`distil-whisper/earnings22`](https://huggingface.co/datasets/distil-whisper/earnings22), `0a034f9ed86d33a3859d9025d3e621cf243773ab` (2023-10-13), `full`, `test` | Full-call ASR, long-form seams, accented English | 125 real calls; `chunked` is a different 57,391-record test. Convenient mirror has plain transcripts rather than complete speaker references. The original text license is CC-BY-SA-4.0; it explicitly covers transcripts/text files. Do not infer blanket audio redistribution rights from that sentence. [Pinned card](https://huggingface.co/datasets/distil-whisper/earnings22/blob/0a034f9ed86d33a3859d9025d3e621cf243773ab/README.md), [original license](https://github.com/revdotcom/speech-datasets/blob/main/earnings22/LICENSE.md). |

The original Earnings22 token format includes speaker IDs, start/end timestamps, punctuation, case, and entity tags, but examples contain missing times. Validate the selected annotations before treating them as word-time truth. Its public telephone/business calls test accented English and long-form recognition, not every far-field overlap condition. Source: [original Earnings22 format](https://github.com/revdotcom/speech-datasets/tree/main/earnings22).

Use AMI and NOTSOFAR for complete-meeting quality with explicit licenses. Add a held-out consenting English/German meeting set for the actual microphone/system mix, switches, proper names, and quiet turns. FLEURS can supplement language coverage but is read speech, not a substitute for that set. Split by complete meeting and speaker/session groups where possible; never tune thresholds on the final decision set.

The following are **download examples for a future evaluator**, not actions performed in this review. Download and materialize local WAV/reference manifests separately from inference. Pin decoder/resampling versions and write content hashes; modern `datasets` audio decoding may use TorchCodec rather than the older `audio["array"]` interface.

```python
# Cropped ASR screening; do not report this as full-meeting diarization.
from datasets import load_dataset
ami = load_dataset(
    "edinburghcstr/ami", "sdm", split="test", streaming=True,
    revision="46f28f2503e2ec48f8867a84eef356c70476beab",
)

# Complete distant meeting files; replace explicit_session_id from the pinned
# dataset tree and retain every reference/audio file for that selected session.
from huggingface_hub import snapshot_download
snapshot_download(
    repo_id="microsoft/NOTSOFAR", repo_type="dataset",
    revision="ba8fd0f034ce185fe4d24f47e53b4b8194795f07",
    allow_patterns=[
        "benchmark-datasets/eval_set/240629.1_eval_small_with_GT/MTG/"
        "explicit_session_id/**"
    ],
    local_dir=public_data_dir,
)
```

Microsoft's helper downloads an unpinned subfolder; wrap the pinned Hub call rather than silently fetching mutable `main`. Use code revision `6f58e08b008f7530ba4141f0aeb02447c70b6fd7` (2025-02-12) when adapting its session/device and reference parsing. Sources: [original download helper](https://github.com/microsoft/NOTSOFAR1-Challenge/blob/6f58e08b008f7530ba4141f0aeb02447c70b6fd7/utils/notsofar_dataset.py), [cutoff commit metadata](https://api.github.com/repos/microsoft/NOTSOFAR1-Challenge/commits?until=2026-09-30T23:59:59Z&per_page=1).

### Portable artifacts and meaningful metrics

Use a single engine-independent JSONL prediction export and a reference manifest. An example schema, proposed for implementation:

```json
{
  "session_id": "heldout-meeting-01-sdm",
  "audio_path": "/private/eval/meeting-01.wav",
  "audio_sha256": "...",
  "sample_rate": 16000,
  "duration_seconds": 1200.0,
  "source_channel": "sdm",
  "language_spans": [{"start": 0.0, "end": 1200.0, "language": "de"}],
  "reference_words_path": "/private/eval/meeting-01.seglst.json",
  "reference_rttm_path": "/private/eval/meeting-01.rttm",
  "uem_path": "/private/eval/meeting-01.uem",
  "split": "heldout"
}
```

Predictions should retain text, language, stable segment ID, start/end, anonymous speaker, overlap/unknown state, and timing provenance. Keep ASR words, regular diarization, exclusive display turns, warnings, and raw generation separately. Do not drop unknown-speaker words when exporting. Assign a documented unknown hypothesis stream for scoring; otherwise missing labels can misleadingly improve the score.

Use [MeetEval](https://github.com/fgnt/meeteval) for meeting WER, with its SegLST interchange format (`session_id`, `speaker`, `words`, `start_time`, `end_time`). cpWER globally permutes speaker labels across the whole session; tcpWER adds time constraints. ORC/time-constrained ORC can evaluate word recovery without forcing a fixed speaker identity assignment. Label permutation is not a reason to rematch independently every minute, which would hide speaker drift. Source: [official metric and format documentation](https://github.com/fgnt/meeteval).

Use [dscore](https://github.com/nryant/dscore) for DER/JER with original regular RTTM and explicit UEM. Its default DER uses zero collar and includes overlap; a second 250 ms-collar view may aid diagnosis, but must be labeled. JER weights speaker segmentation differently and cannot replace missing/false speech or word scores. Preserve missed speech, false alarm, and speaker confusion components. Source: [dscore protocol](https://github.com/nryant/dscore).

Example scoring commands after reference/prediction conversion:

```bash
meeteval-wer cpwer -r reference.seglst.json -h hypothesis.seglst.json
meeteval-wer tcpwer -r reference.seglst.json -h hypothesis.seglst.json --collar 5
python /pinned/dscore/score.py -u scored.uem -r reference.rttm -s hypothesis.rttm --collar 0
```

The five-second tcpWER collar is MeetEval's documented example, not a mandatory Sonefold setting. Freeze it for comparability, report a tighter timing sensitivity analysis, and keep word boundary error separate. MOSS segment times or chunk fallback times cannot fairly be presented as exact word times by duplicating boundaries for every word.

| Question | Minimum measurements |
| --- | --- |
| Are the words recovered? | WER/CER with S/D/I counts; whole-corpus micro score plus per-session distribution; per-language and overlap/non-overlap strata. |
| Are words attributed correctly? | cpWER and tcpWER; an ORC-style word recovery view; unknown assignment rate; speaker count errors and full-session identity drift. |
| Are speakers timed correctly? | Zero-collar overlap-included DER/JER, DER components, optional labeled collar view, overlap recall. |
| Are times useful? | Matched-word start/end absolute error quantiles, alignment coverage, malformed/out-of-range times, seam duplication/deletion. |
| Is silence handled? | Inserted words per hour of nonspeech, hallucinated segment rate, false speech duration, and silent/low-noise/music strata. Empty-reference WER is undefined. |
| Is meeting text usable? | Punctuation/case F1 on agreed labels, number/name/entity accuracy, negation recall, verbatim versus ITN differences. |
| Does streaming remain responsive? | Cold readiness; first partial; last revision/stable word; endpoint lag; speaker-label revision rate; chunk p50/p95/p99; queue backlog/overloads; tail flush and clock error. |
| Can the laptop sustain it? | Peak GPU allocated/reserved and observed process memory, CPU RSS, startup/cache footprint, steady-state RTF and sustained live playback at the actual GPU-sharing cap. |

Compute paired differences on the same sessions and bootstrap complete meetings, not individual correlated words or overlapping crops. Show confidence intervals and failure examples. A ten-minute favorable sample should be labeled a pilot, not a new SOTA result.

### Stage isolation: the most useful ablation matrix

Run the following on the same complete meetings and frozen references:

1. **Recognition only:** raw audio, no diarization, fixed segmentation; native Parakeet/Whisper, original Qwen, NeMo Parakeet, Cohere, and available joint candidates.
2. **Diarization only:** raw audio to Nemotron/Community-1; regular timeline to DER/JER. Reference speech boundaries can isolate the VAD contribution, but label this as oracle speech activity.
3. **Predicted ASR + predicted diarization:** current association and speaker-conditioned variants; score end-to-end cpWER/tcpWER.
4. **Predicted ASR + oracle diarization:** determine how much word recovery/attribution improves when speaker activity is correct.
5. **Reference words + predicted diarization:** isolate assignment/segmentation errors using trusted word times.
6. **Pipeline controls:** raw/RNNoise/optional enhancement; VAD configurations; language auto/known/router; context/hotword/decoy; chunk boundary strategies; alignment on/off and coverage.

The two oracle directions answer different questions. With perfect reference words, excellent attribution does not demonstrate that the ASR recovered those words. With perfect diarization, improved conditioned ASR does not establish deployable diarization accuracy.

For live evaluation, replay the original continuous PCM at wall-clock pace through the actual streaming interface, including silence, overlap, pauses, and the final flush. Store monotonic event times, source audio offsets, request sizes, queue state, and every cumulative snapshot. Derive the final transcript by stable-ID upserts, as the client does, rather than appending snapshots. A retrospectively stable word is one that never changes again before completion; report its latency separately from the first speculative word. Accelerated offline replay measures throughput, not user-visible latency.

Evaluate a small frozen regression set containing: <250 ms backchannels; short initial speech; a long German sentence with early-word retention; a speaker returning after a long absence; two quiet overlapping speakers; English/German switches mid-turn; ≥8-speaker boundary cases; silent/floor-noise stretches; names/numbers; and several chunk seams. Use real recordings for accuracy, with synthetic clips as deterministic integration fixtures.

## Runtime and RTX 5090 Laptop constraints

Weight size is a lower bound, not a VRAM requirement. At two bytes per parameter, 0.6B/0.9B/1.7B/2B/4B/9B imply roughly **1.2/1.8/3.4/4/8/18 GB** of weights before caches, features, activations, allocator reserve, aligners, diarizers, or the desktop. These are arithmetic estimates, not model measurements. Large offline context and simultaneous speakers can dominate additional memory.

| Environment | Verified original interface | Compatibility point |
| --- | --- | --- |
| Existing Qwen worker | `Qwen3ASRModel`, optional `Qwen3ForcedAligner` | Original package's Transformers 4.57.6; keep separate from newer native models. |
| Existing NeMo worker | Cached RNNT/Sortformer integration and original `.nemo` checkpoints | Pin NeMo source/API and checkpoint together. Installed NeMo alone does not establish the needed integration API. |
| Cohere candidate | Native `AutoProcessor` / `AutoModelForSpeechSeq2Seq` | Verified at Transformers 5.4.0, explicit language, local files; no remote code needed. |
| MOSS candidate | Reviewed custom processor/causal model plus parser | Separate Transformers >=5.6,<6 runtime; long context and output budgets need limits. |
| Photon candidates | `moondream.photon(...).transcribe(...)` | Verify actual laptop chip and packaged runtime; source model architecture compatibility does not prove ONNX compatibility. |
| Alignment comparison | Original Qwen or reviewed language-specific acoustic aligner | Treat supported language, text preservation, API removal, and timed coverage as explicit capabilities. |

Runtime sources are cited in the corresponding model sections. Avoid replacing these isolated environments with one blanket `pip install -U`; the dependency contracts differ materially.

When the GPU also drives the desktop, run heavy CUDA benchmark/training commands, including worker/evaluator launches and installation smoke runs that do heavy GPU work, under a GPU-sharing launcher ([GPU notes](../GPU_ACCELERATION.md#sharing-the-gpu-with-the-desktop)). Benchmarks should not change display, GPU-routing or audio-system settings.

For example, the existing benchmark can be run later against a prepared private manifest:

```bash
/path/to/gpu-sharing-launcher /path/to/isolated/python speech-worker/tools/benchmark_asr.py \
  --manifest /private/eval/heldout.jsonl --model qwen3-asr-1.7b \
  --device cuda --output /private/eval/qwen-heldout.jsonl
```

Record any GPU-sharing cap, GPU/driver/CUDA versions, framework wheel, model and source revisions, attention implementation, dtype, batch/context settings, and all competing resident models. Separate cold load/readiness from warmed steady state. Synchronize CUDA before ending timed stages. Report both framework allocation/reserve and whole-process observations; a GPU-sharing daemon can complicate process attribution. Measure sustained audio-paced replay under normal desktop use rather than translating server throughput into desktop latency.

Unload the local summarization LLM during heavy offline transcription unless concurrency is an explicitly measured requirement. Start with one ASR, then alignment, then diarization sequentially where practical. Reuse warm workers within a session but verify cache reset between sessions. The earlier report's long cold-start/warmup times are meaningful readiness costs and should not be hidden inside only the first clip's RTF.

## Promotion criteria and unresolved facts

Promote a candidate only after paired held-out evidence improves the desired meeting outcomes without unacceptable startup, latency, memory, or silent omissions. Keep a Pareto comparison: lower WER with higher stable-word latency is a tradeoff, not automatically a better live default. A useful live preset may differ from offline finalization and CPU fallback.

The most consequential unresolved facts are:

- Real English/German meeting WER/cpWER/DER and overlap recall for the implemented paths.
- Whether the German streaming deletions persist across latency/context profiles and whether oracle speaker activity changes them.
- Cohere's silence, switches, chunk seams, dense-generation truncation, and external alignment on the intended recordings.
- MOSS speaker drift, malformed-tail rate, word-time coverage, and bounded-context memory on the laptop.
- Photon support and single-session performance on RTX 5090 Laptop; model-weight licensing does not settle runtime packaging.
- Complete reference provenance and licenses for every downloaded benchmark component, particularly avoiding a blanket audio-license inference for Earnings22.

No evidence in this review identifies a universal best ASR/diarizer or a measured winner on the target hardware. The missing newer candidates are now concrete, and the evaluator above would make the next choice reviewable rather than dependent on incompatible leaderboard averages.

## API clarification: Photon Ultra and Cohere access

Follow-up primary-source audit, still within the 2026-09-30 cutoff. Only small public source distributions and metadata were read; no runtime was installed or imported and no model inference was run.

### Photon: pinned local artifacts and exact output contract

The cutoff SDK is **`moondream==2.6.1`**, published 2026-09-30 00:46 UTC. Its wheel SHA256 is `73adcfa8bd701a78f589b8e4b56696fbd305486c3d9bf8856f9bbb759f7d0ff4`. It requires **`kestrel==0.9.1`**, whose wheel SHA256 is `80640639279b18b2da61b08c06c550f8eaab6596d6d15978fd56f76707462cb7`. Kestrel requires PyTorch >=2.8 on this x86_64 machine, `kestrel-native==0.1.8`, and `kestrel-kernels==0.7.4`. Python must be >=3.10,<3.15. Sources: [SDK release metadata](https://pypi.org/pypi/moondream/2.6.1/json), [runtime release metadata](https://pypi.org/pypi/kestrel/0.9.1/json).

`md.photon(model, **runtime_config)` delegates to Kestrel's `RuntimeConfig`. That configuration supports local `model_path`, device/dtype, graph/decode policy, and batch capacity, but **does not have a `revision` or `local_files_only` argument**. Ultra/Redux registry entries are unpinned and the loader defaults to mutable `main`; the original NVIDIA Parakeet entry is pinned. A Sonefold adapter should download an explicit revision separately and load those local files. Source: [original revision-contract tests](https://github.com/m87-labs/kestrel/blob/d00b3490f46e2e8b35a880e6511f5eb6b5980198/tests/asr/test_parakeet_model_ids.py), [runtime configuration](https://github.com/m87-labs/kestrel/blob/d00b3490f46e2e8b35a880e6511f5eb6b5980198/kestrel/config.py).

Example preparation, **not executed**:

```python
from huggingface_hub import snapshot_download

snapshot_dir = snapshot_download(
    "moondream/parakeet-ultra",
    revision="73175eb7aeb0d82f1e2a6b53b3aabc10a90bcd0b",
    allow_patterns=["config.json", "model.safetensors", "tokenizer.json"],
    local_dir=download_directory,
    token=False,
)
```

At that revision the model repository contains those three required files; the safetensors file is approximately 1.26 GB. Standard Hub downloads use `HF_HOME` (normally `~/.cache/huggingface`). Preserve the downloaded snapshot and file hashes, since a remote owner can rewrite history. Sources: [pinned artifact tree](https://huggingface.co/api/models/moondream/parakeet-ultra/tree/73175eb7aeb0d82f1e2a6b53b3aabc10a90bcd0b?recursive=true), [Kestrel downloader](https://github.com/m87-labs/kestrel/blob/d00b3490f46e2e8b35a880e6511f5eb6b5980198/kestrel/model_download.py).

Example inference after preparation, **not executed**. The worker needs the egress isolation described below; the environment flag alone does not make Photon network-free. On a GPU that also drives the desktop, run heavy CUDA launches under a GPU-sharing launcher.

```python
import os
from pathlib import Path

# Configure before importing the runtime. This blocks Hub download/probe calls.
os.environ["HF_HUB_OFFLINE"] = "1"
os.environ.pop("MOONDREAM_API_KEY", None)

import torch
import moondream as md

checkpoint_dir = Path(local_snapshot_directory)
with md.photon(
    "moondream/parakeet-ultra",
    device="cuda",
    dtype=torch.bfloat16,
    model_path=str(checkpoint_dir / "model.safetensors"),
    max_batch_size=1,
    single_pass_batch_capacity=1,
    decode_path="auto",
) as speech:
    result = speech.transcribe(audio=Path(local_audio_path), timestamps="word")
    text = result["text"]
    for segment in result["segments"]:
        segment_start, segment_end = segment["start"], segment["end"]
        for word in segment.get("words", []):
            print(word["start"], word["end"], word["word"])
```

The local loader accepts a directory or file and resolves sibling `config.json`, `model.safetensors`, and `tokenizer.json`; keep them together. Although the general configuration exposes `tokenizer_path`, the Parakeet loader reads the sibling tokenizer. The returned **segment** contains `start`, `end`, `text`, and nested `words`; word records contain `start`, `end`, and `word`. This clarifies the earlier abbreviated description of word timestamps. Sources: [checkpoint resolver](https://github.com/m87-labs/kestrel/blob/d00b3490f46e2e8b35a880e6511f5eb6b5980198/kestrel/models/asr/checkpoint.py), [Parakeet loader](https://github.com/m87-labs/kestrel/blob/d00b3490f46e2e8b35a880e6511f5eb6b5980198/kestrel/models/parakeet_tdt/weights.py), [model card's output example](https://huggingface.co/moondream/parakeet-ultra/blob/73175eb7aeb0d82f1e2a6b53b3aabc10a90bcd0b/README.md).

Encoded audio is decoded/downmixed by the runtime. Raw mono NumPy/CPU Torch PCM requires an explicit sample rate. Parakeet uses automatic recognition but does not report a detected language code; it rejects forced language, translation, initial prompt, and stochastic decoding options. Do not forward Qwen/Cohere language settings indiscriminately. Sources: [original Parakeet request validator](https://github.com/m87-labs/kestrel/blob/d00b3490f46e2e8b35a880e6511f5eb6b5980198/kestrel/models/parakeet_tdt/contract.py), [speech API documentation](https://docs.moondream.ai/transcription/).

The runtime defaults to capacity 64 on CUDA devices below 40 GiB and 128 above; CPU/MPS default to 8. That capacity is separate from `max_batch_size`. The example deliberately limits both for a single meeting, to avoid unnecessary large graph/decoder allocation. This is a proposed starting configuration, not a measured optimum. `decode_path="auto"` attempts generated decode and can use a native alternative; `generated` is fail-closed and requires CUDA BF16. Sources: [capacity tests](https://github.com/m87-labs/kestrel/blob/d00b3490f46e2e8b35a880e6511f5eb6b5980198/tests/asr/test_parakeet_batch_capacity.py), [generated decode construction](https://github.com/m87-labs/kestrel/blob/d00b3490f46e2e8b35a880e6511f5eb6b5980198/kestrel/models/parakeet_tdt/generated_decode.py).

The common Parakeet implementation includes a CPU branch for full-precision weights and CPU thread/dtype selection. Thus a `device="cpu"`, `dtype=torch.float32`, bounded-capacity Ultra trial is supported by the source structure, but **was not run or certified here**. Official docs position Ultra for GPUs and Redux for CPU/Apple efficiency. The named production GPU matrix includes RTX PRO 6000 Blackwell, not RTX 5090 Laptop. Architecture similarity does not establish exact laptop kernel compatibility or performance. Sources: [shared runtime](https://github.com/m87-labs/kestrel/blob/d00b3490f46e2e8b35a880e6511f5eb6b5980198/kestrel/models/parakeet_tdt/runtime.py), [original ASR documentation](https://github.com/m87-labs/kestrel/blob/d00b3490f46e2e8b35a880e6511f5eb6b5980198/docs/asr.md), [named hardware matrix](https://moondream.ai/photon/support).

### Photon local computation still reports usage metadata

The audited runtime creates a reporter even without an API key and attempts startup, periodic, and shutdown telemetry. Report fields include hostname, model, GPU/memory description, timestamps, and aggregate counters. The source does not put audio/transcript contents in that report, but this is still external metadata traffic. `HF_HUB_OFFLINE` only addresses Hub operations. Source: [reporter implementation](https://github.com/m87-labs/kestrel/blob/d00b3490f46e2e8b35a880e6511f5eb6b5980198/kestrel/photon.py).

There is no telemetry-off field in the reviewed public configuration. `MOONDREAM_API_BASE_URL` is **whitelisted to two cloud endpoints**; setting it to localhost is silently replaced with the default cloud endpoint. Therefore a strict-local worker needs actual process egress isolation or a separately reviewed source change. Do not claim that an arbitrary loopback URL or an unset API key disables reporting. Sources: [engine startup and endpoint whitelist](https://github.com/m87-labs/kestrel/blob/d00b3490f46e2e8b35a880e6511f5eb6b5980198/kestrel/engine/core.py), [configuration fields](https://github.com/m87-labs/kestrel/blob/d00b3490f46e2e8b35a880e6511f5eb6b5980198/kestrel/config.py).

The model weights are CC-BY-4.0. Free local runtime use is documented, but the inspected runtime package metadata has no license expression and the requested repository `LICENSE` path returned 404. This audit does not establish permission to redistribute or modify runtime components under the weight license. An optional installed dependency and a vendored runtime are different packaging decisions. Sources: [weight license](https://huggingface.co/moondream/parakeet-ultra), [runtime package metadata](https://pypi.org/pypi/kestrel/0.9.1/json).

### Cohere: automatic gate approval still requires user acceptance

An anonymous request for the pinned original `config.json` returned **HTTP 401, `X-Error-Code: GatedRepo`**, explicitly requiring authorized authenticated access. Public `gated: "auto"` metadata means access is approved automatically **after** a logged-in user submits the access request. It does not enable anonymous `snapshot_download`. Sources: [original metadata](https://huggingface.co/api/models/CohereLabs/cohere-transcribe-03-2026), [pinned gated configuration endpoint](https://huggingface.co/CohereLabs/cohere-transcribe-03-2026/resolve/b1eacc2686a3d08ceaae5f24a88b1d519620bc09/config.json).

Hugging Face requires browser access-request acceptance sharing the user's username/email with the model owner, then an authenticated token for scripted downloads. Cohere's public metadata exposed no additional custom form fields in this audit; the actual logged-in form should be reviewed before accepting. If the account already has accepted access, an existing authorized read token is sufficient. The adapter should surface an access-required error and continue useful evaluation of available original models; it should not silently accept contact sharing or substitute a converted checkpoint. Source: [official gated-model access procedure](https://huggingface.co/docs/hub/models-gated#access-gated-models-as-a-user).


## Native Qwen runtime follow-up

The [huanglizhuo/QwenASR](https://github.com/huanglizhuo/QwenASR) Rust implementation supports both 0.6B and 1.7B, streaming, alignment and offline output. Its published speed comparison targets the 0.6B model on Apple M5 Pro; it does not establish RTX 5090 inference performance. Sonefold's current CUDA runtime already loads Qwen3-ASR1.7B and the optional forced aligner. Fresh capable systems recommend 1.7B (at least 16 GiB RAM or roughly 6 GiB CUDA VRAM), while smaller systems recommend 0.6B. Existing explicit overrides survive initialization. Model changes clear a checkpoint path belonging to a different model, and settings show the hardware recommendation. Native-runtime migration would require matched quality and performance measurements before replacing this working CUDA path.
