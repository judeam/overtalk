# Local meeting ASR evidence inventory

Date/cutoff: 2026-09-30. Status: complete primary-source screening.

Search terms: local ASR, streaming, multilingual, model release, license, timestamps, code-switching, NeMo, Qwen3-ASR, Granite Speech, Voxtral, Moonshine.

Screening rubric: meeting fit (0–3), deployment/API data (0–4), verified release/license specificity (0–3); relevant threshold 7. This review tracks model cards, runtime documentation, packages and papers separately rather than presenting all sources as research papers. 31 primary sources inventoried; every inventoried source met the relevance threshold. Two directly linked paper references recorded in the citation graph. The source review itself contains no model-quality or GPU performance measurements. Separate real [CPU](real-cpu-smoke.json) and [CUDA](real-gpu-smoke.json) smoke artifacts validate synthetic English/German Qwen adapter/alignment behavior with pinned weights; CUDA ran under a GPU-sharing launcher at 85% of GPU threads. Meeting accuracy, throughput, peak memory, and NeMo/pyannote GPU compatibility remain unmeasured.

[Synthesis and reproducible benchmark methodology](../2026-09-30-stt.md). Manufacturer accuracy claims remain labelled; shared benchmark scripts provide the reproducible comparison path.
