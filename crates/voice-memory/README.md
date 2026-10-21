# Local voice memory

This crate is Apache-2.0 licensed. It computes voice embeddings with an explicitly
CPU-only ONNX session (two intra-op threads, one inter-op thread). A speaker needs
at least two clean, non-overlapping speech samples. At most three 3-second samples
are read for each speaker; the complete meeting is never decoded into memory.
The model/session is released when the operation ends. No idle polling or cloud
inference is involved.

The app stores a name and up to three normalized 192-dimensional embeddings in
its local SQLite database only after **Remember voice**. These are sensitive voice
profiles, not authentication credentials. **Remembered voices** lets the user
delete them. Deletion prevents future recognition and leaves historical meeting
names intact. Renaming a meeting speaker does not silently enroll that person.

Recognition requires both a minimum cosine similarity of 0.65 and a runner-up
margin of 0.12, with consistent evidence from multiple samples. Scores are not
identity probabilities. These conservative defaults can leave known people
unnamed when microphones, noise, language or delivery change. The interface marks
automatic names as recognized voices so they can be checked and corrected.

## Model provenance

- Original model: [3D-Speaker / CAM++](https://www.modelscope.cn/models/iic/speech_campplus_sv_zh-cn_16k-common), Apache License 2.0.
- Same publisher's [FunASR model card](https://huggingface.co/funasr/campplus) explicitly identifies Apache-2.0 and 192-dimensional, 16-kHz input.
- CPU ONNX conversion: [sherpa-onnx speaker recognition release](https://github.com/k2-fsa/sherpa-onnx/releases/tag/speaker-recongition-models).
- Artifact: `3dspeaker_speech_campplus_sv_zh-cn_16k-common.onnx`.
- SHA-256: `f682b514c05d947ee3fa91cd6ec6c5c7543479a128373fa29b1faedccd21fd11`.
- The conversion embeds the original ModelScope URL, input sample rate, feature
  normalization and embedding size in its ONNX metadata. The app checks the full
  SHA-256 before loading and rejects oversized downloads. No Python, pickle,
  remote model code, NeMo or CUDA runtime is needed for this feature.
- The original authors are the 3D-Speaker/Alibaba DAMO Academy team. Keep this
  attribution and the accompanying Apache license when redistributing the model.

`fbank` implements the published Kaldi-style frontend. A local comparison against
the publisher's `kaldi-native-fbank` reference produced embedding cosine similarity
above 0.9999. The benchmark example reads bounded, raw f32 little-endian 16-kHz
mono samples for repeatable CPU speed and memory measurement.

## Validation

`cargo test -p overtalk-voice-memory` checks conservative rejection, cross-talk
exclusion and invalid-input handling. The native service integration test also
checks enrollment, later recognition, profile deletion and cross-process library
refresh using a disposable database. Synthetic speakers do not establish accuracy
for real people, languages, accents or recording equipment.
