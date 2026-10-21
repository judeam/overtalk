# Qwen3.8 27B quantization review — 30 September 2026

## Selection

Use **Agention Precision IQ4_XS** as the default: 14,252,844,960 bytes (13.27 GiB), 24.88% smaller than the previously installed ggml-org Q4_K_M (18,973,870,528 bytes). This keeps the original Qwen model and standard llama.cpp tensor types. It is a quality/memory compromise, not a claim of lossless compression or a universal quantization winner.

| Candidate | Exact bytes | Intended role |
| --- | ---: | --- |
| Agention AP-IQ4_XS | 14,252,844,960 | Default; calibrated four-bit mixed precision |
| Unsloth Dynamic v3 IQ4_XS | 14,252,845,984 | Same-size reference from a widely deployed quantizer |
| ISTA GSQ-RCO IQ3_S | 11,771,546,784 | More compact optional profile |
| Unsloth Dynamic v3 Q4_K_XL | 17,559,178,144 | Higher precision and larger memory cost |
| ggml-org Q4_K_M | 18,973,870,528 | Existing locally validated reference; retained |

Agention publishes paired next-token KL-divergence results on three corpora. AP-IQ4_XS reports smaller divergence than Unsloth's equal-size IQ4_XS on each. The private technical corpus cannot be independently reproduced; the neutral web corpus and WikiText protocol are public. These are the quantizer's results, not Sonefold's measurements or meeting-factuality scores. [Primary model card](https://huggingface.co/agentionai/Qwen3.8-27B-AP-GGUF).

Unsloth Dynamic v3 combines varied importance-matrix calibration with selective per-layer precision. Its published evaluation includes held-out multi-token divergence, beyond a single-token accuracy statistic. Its own guidance warns that very small one-bit variants can loop, omit responses or break tool calls. Sonefold therefore does not select the smallest file regardless of behavior. [Primary methodology](https://unsloth.ai/docs/basics/dynamic-3.0-ggufs), [released files](https://huggingface.co/unsloth/Qwen3.8-27B-GGUF).

ISTA's GSQ-RCO allocates tensor precision through a constrained gradient-based search, using standard GGUF formats. Its recommended IQ3_S reports nearly unchanged averages on three reasoning/generation tasks at 11.77 GB. That does not establish unchanged quotation accuracy, multilingual meeting summaries or long-context factuality. It remains an explicit compact alternative. [Primary model card](https://huggingface.co/ISTA-DASLab/Qwen3.8-27B-GSQ-RCO-GGUF).

## Reproducibility

Default: `agentionai/Qwen3.8-27B-AP-GGUF`, revision `fdb2d87d4f730dd4c395156cf3361acd0d045033`, file `Qwen3.8-27B-AP-IQ4_XS.gguf`, SHA-256 `2074fd5c3c7f6540913c2f62ad02c50b3f7dde7880d18b3acb02432f2edcab67`.

Compact: `ISTA-DASLab/Qwen3.8-27B-GSQ-RCO-GGUF`, revision `d562806dbafae37109975e970aae91b43e73b440`, file `Qwen3.8-27B-GSQ-RCO-IQ3_S.gguf`, SHA-256 `64b53b64c7aa39f20a7e54bd80582fe595b1d745624ee8a72e92508c0326d810`.

The app pins revisions and verifies downloads. Text workflows do not download the separate vision projector or draft/MTP checkpoint. All profiles keep a bounded 16K context, serialize heavy model work, and preserve desktop GPU headroom. The slimmer default allows a more capable recommendation on smaller RAM/VRAM tiers; capacity does not guarantee fast CPU decoding.

## Local validation

The default file passed exact byte-size and SHA-256 verification. AP-IQ4_XS and the old Q4_K_M each passed five synthetic English/German evidence, abstention and injection fixtures through the real CUDA helper under a GPU-sharing launcher at 85% of GPU threads. The returned claims were inspected as well as their exact quotes. See [recorded results](2026-09-30-quantization-smoke.json).

At 16K context, the calibrated runtime reported 12,578.19 MiB for the GPU model buffer, 1,024 MiB for KV, 146.51 MiB recurrent state and 505 MiB compute. These are reported buffers, not peak VRAM including the driver and desktop. The runtime offloaded 65/66 layers; the separate optional draft head is unused. Cold-start fixture time was 47.9 seconds versus 53.0 for the reference in this sequential run. Those few measurements do not establish a general speed ranking.

The compact IQ3_S file also passed its exact size and SHA-256 check and all five synthetic CUDA evidence/abstention/injection fixtures. No full BF16 fidelity, corpus-scale summary benchmark or universal quantization superiority is claimed.
