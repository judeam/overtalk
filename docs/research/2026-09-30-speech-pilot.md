# Matched speech pilot — 30 September 2026

Qwen3-ASR 1.7B made fewer word errors than Whisper Large v3 Turbo on this small matched set. That supports keeping Qwen as this installation's default; it does not establish overall superiority. MOSS remains experimental because its structured output was rejected on two clips.

| Engine | Accepted clips | All-clip WER | CER | S / D / I |
| --- | --- | --- | --- | --- |
| Qwen3-ASR 1.7B | 24/24 | 43.13% | 31.46% | 24 / 43 / 24 |
| Whisper Large v3 Turbo | 24/24 | 49.29% | 40.52% | 33 / 40 / 31 |
| MOSS Transcribe Diarize | 22/24 | 63.03% | 54.81% | 30 / 44 / 59 |

All rows use the same 211 reference words. For the operational all-clip metric, rejected MOSS outputs count as empty predictions rather than disappearing from evaluation. Its accepted-only WER is 55.93% over 177 words; that subset cannot be ranked against the other full-set rows. The strict parser preserves the prior transcript when output is malformed.

## Protocol and reproducibility

The public [AMI dataset](https://huggingface.co/datasets/edinburghcstr/ami), CC BY 4.0, is recorded at revision `46f28f2503e2ec48f8867a84eef356c70476beab`. We used eight short utterances each from EN2002a, EN2002b and EN2002d at fixed test offsets 0, 2000 and 4000. These are deterministic convenience samples, not random or representative meetings. Audio was obtained from the dataset viewer; its rendered audio API is not a pinned parquet download. Every local audio file is hashed in the private manifest. Aggregate scores and manifest hash are [recorded here](2026-09-30-speech-pilot.json); audio, references and raw predictions stay outside Git.

Qwen uses checkpoint `7278e1e70fe206f11671096ffdd38061171dd6e5`; MOSS uses `704aa4a9c304e8520be88901e0d1960158ef5b15`. Whisper uses `mobiuslabsgmbh/faster-whisper-large-v3-turbo` revision `0a363e9161cbc7ed1431c9597a8ceaf0c4f78fcf`, faster-whisper 1.2.1 / CTranslate2 4.8.2, CPU float32, beam size 5, no VAD or previous-text conditioning. SoundFile supplies waveforms because the installed PyAV 19 interface differs from faster-whisper's file decoder. That evaluation path is not a shipped speech adapter.

Qwen/MOSS use CUDA on an RTX 5090 Laptop GPU under a GPU-sharing launcher at 85% of GPU threads; Whisper uses CPU. Consequently the recorded elapsed medians cannot establish a relative-speed claim. Model loading and inference are distinguished in local logs.

Scoring uses jiwer 4.0.0 with NFKC, case folding and punctuation-to-space normalization. CER removes spaces after normalization. This is not the Open ASR Leaderboard's complete normalization. `speech-worker/tools/score_predictions.py` rejects missing, duplicate and unknown predictions and reports failures separately. `score_speakers.py` additionally supports whole-meeting cpWER, tcpWER, DER and JER with declared collars/overlap and approximate word timing; corpus evaluation-region support is not implemented, so it cannot reproduce every official scoring protocol.

## Limits and next evaluation

There is no continuous meeting DER/JER, German real-meeting evaluation, silence challenge set, timestamp accuracy, confidence calibration or significance interval in this pilot. Zero silence hallucinations in the JSON does not certify silence robustness: these samples contain speech. The high error rates and tiny denominator rule out a broad accuracy claim. Compare continuous held-out English/German meetings, overlapping voices, multiple microphones and matched hardware before changing this conclusion. Gated Cohere weights remain untested pending acceptance of the upstream access terms.
