# Attribution and third-party notices

Overtalk's own copyright and MIT terms are in [LICENSE.md](LICENSE.md). Additional copyright and license notices for inherited and third-party components are preserved below. Both files are bundled with the application.

Third-party components keep their own licenses, listed below.

## Inherited MIT-licensed portions

Overtalk began as a fork of the community edition of
[Meetily](https://github.com/Zackriya-Solutions/meetily) (v0.4.1) by Zackriya
Solutions. The following notice applies to retained upstream code and components
derived from it. It does not identify the owner of Overtalk-specific additions.

```text
MIT License

Copyright (c) 2024 Zackriya Solutions

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```

## Models

Model weights are downloaded separately and are not part of this repository or its MIT license. Review the source model's terms before downloading or redistributing weights.

- Qwen3-ASR and its forced aligner: [Qwen](https://huggingface.co/Qwen/Qwen3-ASR-1.7B), Apache 2.0.
- Parakeet v3: [NVIDIA](https://huggingface.co/nvidia/parakeet-tdt-0.6b-v3); [Istupakov's ONNX conversion](https://huggingface.co/istupakov/parakeet-tdt-0.6b-v3-onnx), CC BY 4.0. The app uses conversion revision `8f23f0c03c8761650bdb5b40aaf3e40d2c15f1ce`.
- Parakeet v2 (English): [NVIDIA](https://huggingface.co/nvidia/parakeet-tdt-0.6b-v2); [Istupakov's ONNX conversion](https://huggingface.co/istupakov/parakeet-tdt-0.6b-v2-onnx), CC BY 4.0.
- Whisper GGML weights from [whisper.cpp](https://huggingface.co/ggerganov/whisper.cpp): MIT.
- Notes models: Qwen3.5 2B and 4B (GGUF by Unsloth) and Gemma 4 E2B and E4B (Google's QAT GGUF) are Apache 2.0. Gemma 3 1B and 4B (GGUF by bartowski) are provided under and subject to the [Gemma Terms of Use](https://ai.google.dev/gemma/terms), including its Prohibited Use Policy.
- Nemotron checkpoints use their upstream NVIDIA terms; see the pinned cards in [the diarization report](docs/research/2026-09-30-diarization.md).
- Pyannote Community-1 requires accepting the model's upstream access conditions. It is an optional separately downloaded dependency.
- Other Whisper, GGUF and speech checkpoints retain their individual licenses.
- Qwen3.8 27B summaries and Qwen3 embedding/reranker checkpoints use Apache 2.0 model notices. The Nemotron 3 Embed 8B quality profile uses OpenMDW 1.1. Cohere Transcribe and MOSS weights use their model repository notices. These separately downloaded weights are excluded from the MIT code and installers.
- The calibrated 27B profiles use Agention Precision IQ4_XS and ISTA-DASLab GSQ-RCO IQ3_S GGUF weights, each under its upstream Apache 2.0 notice. See [pinned provenance](docs/research/2026-09-30-quantization-review.md). Weights are separate downloads.

## Fonts and brand assets

Instrument Sans, Phudu and the Noto export fonts are bundled with their SIL Open Font License notices under `frontend/public/fonts/`. Dependency packages retain their own licenses.

The current Overtalk mark and asset sources are documented in [brand provenance](docs/brand/README.md). Product screenshots use synthetic meeting data.

## Vendored speech-worker code

The optional Python speech worker vendors two Apache-2.0 inference runtimes, each
with its license and notice alongside it; files changed for Overtalk say so in
their headers:

- `speech-worker/sonefold_speech/vendor/qwen_asr/`: Qwen3-ASR inference code from
  qwen-asr 0.0.6, Copyright 2026 The Alibaba Qwen team; its vLLM backend also
  carries vLLM, EleutherAI and Hugging Face notices.
- `speech-worker/sonefold_speech/vendor/moss/`: an OpenMOSS MOSS-Transcribe-Diarize
  inference subset, source revision `61bc29cd4120be7b5d3b761b64cd5dff57263642`
  (see its `PROVENANCE.md`).

## Compiled-in components

The application binaries also include Rust crates and C/C++ libraries, listed in
`Cargo.lock`, under their own permissive terms (MIT, Apache-2.0, BSD, ISC, Zlib,
Unicode-3.0, BSL-1.0 and CDLA-Permissive-2.0) and, unmodified, under MPL-2.0
(Symphonia, cssparser, selectors and others; source on crates.io at the locked
versions). Notably:

- The Silero VAD model, embedded through silero-rs: MIT, Copyright (c) 2020-present
  Silero Team ([silero-vad](https://github.com/snakers4/silero-vad)).
- Slint, under the Slint Royalty-free Desktop, Mobile, and Web Applications
  License 2.0 (`native/assets/LICENSE.slint.md`). The native About dialog shows the
  required "About Slint" attribution.
- The shadcn/ui components in `frontend/src/components/ui/`: MIT, Copyright (c) 2023
  shadcn.
- Lucide icons in the native interface: ISC, Copyright (c) Lucide Contributors and,
  for portions from Feather, Cole Bemis (`native/assets/LICENSE.lucide`).

## Voice memory

The `crates/voice-memory` implementation and `frontend/src-tauri/src/extensions/voice_memory.rs`
are Copyright 2026 Jude Mingay, licensed under Apache-2.0 (`crates/voice-memory/LICENSE`).
Its optional CAM++ model is from Alibaba's 3D-Speaker project, licensed Apache-2.0,
converted to ONNX by sherpa-onnx. See `crates/voice-memory/README.md` for the exact
artifact, upstream links and SHA-256. These notices do not change the license of
the rest of Overtalk.

## Native workspace integrations

Optional screen capture uses CapSoftware's `scap` 0.0.8 under MIT. Credential
storage uses `keyring` 3.6.3 under MIT or Apache-2.0. Their notices are retained in
`native/assets/LICENSE.scap`, `LICENSE.keyring-MIT` and
`LICENSE.keyring-Apache-2.0`, and the native Linux installer includes them.

## Evaluation tooling

The AMI word-error-rate scorer in `evaluations/ami/vendor/whisper_normalizer/` is
OpenAI Whisper's English text normaliser (openai-whisper 20250625), MIT, with its
license alongside it. It is not part of the application. The AMI Meeting Corpus
(University of Edinburgh et al.) is CC BY 4.0 and is never stored in this repository.
