# Qwen ASR cold-start latency — 1 October 2026

The retranscription dialog's `1.7s` described segment duration. Its first segment also included unreported Torch/CUDA/model loading and Qwen forced-aligner loading. Native import and retranscription previously stopped the worker after every job. The VAD provider discarded word timestamps, so alignment added work without changing the saved timing precision.

## Changes

- Prepare the model during the loading stage, using the job's cancellation token.
- Keep successful batch workers warm for 120 idle seconds. Diagnostic checks do not extend this timeout. A stale idle timer cannot terminate a recently used worker.
- Release workers immediately after errors/cancellation, settings changes, and before summary or quality retrieval inference. Recording startup shares the lifecycle lock with idle cleanup.
- Request `word_timestamps:false` from the text-only VAD provider. Full analysis keeps word timestamps; speaker analysis rejects disabling them.
- Label segment duration as seconds **of audio**.

## Measurements

RTX 5090 Laptop, Qwen3-ASR 1.7B, BF16, CUDA 13/PyTorch 2.14, SDPA. All CUDA runs used a GPU-sharing launcher capped at 85% of GPU threads, exposing 68 SMs. Input was a generated English 1.7-second clip; no real recording was read.

| Measurement | Seconds |
| --- | ---: |
| Original cold model load | 4.686 |
| Original first clip, including aligner loading | 1.979 |
| Original warm clips | 0.099–0.135 |
| Updated text-only cold model load | 4.715 |
| Updated text-only first clip | 1.508 |
| Updated text-only warm clips | 0.135–0.152 |
| Real stdio worker: six clips, cold job | 6.581 |
| Same worker: six clips, second job | 0.623 |

Both protocol jobs returned identical text for every clip. The second `load` reused the model. These are startup/adapter measurements, not end-to-end retranscription timing or an accuracy benchmark. Filesystem caches were not flushed. The warm-clip variation does not establish an inference speedup; avoiding repeated cold starts is the main improvement. Concurrent compilation raised another cold load to 11.089 seconds, illustrating sensitivity to machine load.

The packaged desktop build was also tested in a separate, initially empty profile and DBus session. An 8.841-second synthetic recording imported cold in 13.154 seconds, then retranscribed warm in 0.707 seconds, including decoding, VAD and saving. Health confirmed the same Qwen model remained loaded before/after retranscription. Progress showed model loading separately and identified segment duration as audio length. At 90 idle seconds the model was still loaded; at 125 seconds it had been released, even after the intervening health check. The temporary app exited cleanly; the separately running app and its meetings were untouched. This one short clip establishes cache behavior, not general meeting performance.

## Verification

The new text-only protocol regression failed before the fix and passed afterward. Worker suite: 65 tests, two optional evaluation dependency skips in the Torch environment. Native extensions: 35 passed, including real child-process reuse/idle-expiry behavior. Retranscription: 15 passed. Import: 16 passed, one pre-existing ignored test.

`speech-worker/tools/profile_latency.py` reproduces model-load, first-inference and warm timings from an explicit local audio file. It records timings/runtime metadata without transcript text. [Evidence](stt-evidence/2026-10-01-asr-latency.json) contains synthetic-only measurements.

The CUDA-enabled production frontend/native build and AppImage packaging completed successfully. Both `main` and `devtest` CI runs for code commit `c0d08265a7de5ca032a84ee99a3e999b87b68e7f` passed.
