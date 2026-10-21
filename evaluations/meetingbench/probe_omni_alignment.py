"""CPU-only alignment replay; preserve Qwen text and all baseline artifacts.

Use --backend reference in the isolated Omni/fairseq2 environment, or
--backend transformers in the Qwen environment for the portable adapter.
"""
import argparse
import hashlib
import json
import math
import os
from pathlib import Path
import sys
import time
import unicodedata
import wave

ROOT = Path(__file__).resolve().parents[2]
RUNTIME = ROOT / "evaluations/meetingbench/.runtime"
sys.path.insert(0, str(ROOT / "speech-worker"))


def peak_rss():
    if sys.platform == "win32":
        return None
    import resource
    return resource.getrusage(resource.RUSAGE_SELF).ru_maxrss * (1 if sys.platform == "darwin" else 1024)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--ids", nargs="+", required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--backend", choices=("reference", "transformers"), default="reference")
    args = parser.parse_args()
    if args.output.exists():
        parser.error("Choose a new output directory; previous experiments are immutable")
    model_dir = RUNTIME / "omni-ctc-300m"
    provenance = json.loads((model_dir / "provenance.json").read_text())
    for name in ("omniASR-CTC-300M.pt", "omniASR_tokenizer.model"):
        with (model_dir / name).open("rb") as stream:
            actual = hashlib.file_digest(stream, "sha256").hexdigest()
        if actual != provenance["files"][name]["sha256"]:
            raise ValueError("Alignment checkpoint/tokenizer checksum changed")
    args.output.mkdir(parents=True)
    assets = args.output.resolve() / "assets"
    assets.mkdir()
    user_assets = args.output.resolve() / "user-assets"
    user_assets.mkdir()
    (assets / "local.yaml").write_text(
        "name: overtalk_omni_ctc_v1_local\nmodel_family: wav2vec2_asr\nmodel_arch: 300m\n"
        f"checkpoint: {(model_dir / 'omniASR-CTC-300M.pt').as_uri()}\n"
        "tokenizer_ref: overtalk_omni_tokenizer_v1_local\n---\n"
        "name: overtalk_omni_tokenizer_v1_local\ntokenizer_family: char_tokenizer\n"
        f"tokenizer: {(model_dir / 'omniASR_tokenizer.model').as_uri()}\n")
    os.environ.update(FAIRSEQ2_ASSET_DIR=str(assets), FAIRSEQ2_USER_ASSET_DIR=str(user_assets),
                      FAIRSEQ2_CACHE_DIR=str(args.output.resolve() / "cache"),
                      HF_HUB_OFFLINE="1", TRANSFORMERS_OFFLINE="1", WANDB_MODE="disabled")
    import numpy as np
    import torch
    from scipy.signal import resample_poly
    from sonefold_speech.asr import restore_aligned_text, word_segments, join_word_text
    from sonefold_speech.diarization import assign_speakers
    from sonefold_speech.__main__ import split_speaker_turns
    from .compare_turn_guidance import text_assignment
    from .ctc_alignment import align_tokens

    torch.set_num_threads(4)
    torch.set_num_interop_threads(1)
    started = time.perf_counter()
    if args.backend == "reference":
        from fairseq2.nn.batch_layout import BatchLayout
        from omnilingual_asr.models.inference.pipeline import ASRInferencePipeline
        pipe = ASRInferencePipeline(model_card="overtalk_omni_ctc_v1_local", device="cpu", dtype=torch.float32)
        encoder, decoder = pipe.tokenizer.create_encoder(), pipe.tokenizer.create_decoder(skip_special_tokens=True)
    else:
        from sonefold_speech.omni_alignment import OmniAlignment
        portable = OmniAlignment(model_dir)
    (args.output / "configuration.json").write_text(json.dumps(dict(
        model=provenance, backend=args.backend, resampler="scipy-resample-poly", ids=args.ids, cpu_threads=4,
        load_seconds=time.perf_counter() - started,
        torch_version=torch.__version__,
        code_sha256={name: hashlib.sha256((ROOT / "speech-worker/sonefold_speech" / name).read_bytes()).hexdigest()
                     for name in ("omni_alignment.py", "ctc.py", "asr.py", "diarization.py")},
        limitation="Calibration only; alignment does not validate recognized words or speaker identity"), indent=2))
    for key in args.ids:
        if Path(key).name != key or key in {".", ".."}:
            raise ValueError("Invalid case ID")
        source = json.loads((RUNTIME / "results" / key / "transcript.json").read_text())
        reference = json.loads((RUNTIME / "corpus" / key / "reference.json").read_text())
        turns = [dict(speaker=s, start=a, end=b) for s, a, b in source["turns"]]
        aligned = []
        errors = []
        started = time.perf_counter()
        with wave.open(reference["audio_path"]) as audio:
            if audio.getsampwidth() != 2 or audio.getnchannels() != 1:
                raise ValueError("Replay expects the corpus's mono PCM16 wave files")
            rate = audio.getframerate()
            for index, row in enumerate(source["rows"]):
                try:
                    start, end = row["audio_start_time"], row["audio_end_time"]
                    if not 0 < end - start <= 30.001:
                        raise ValueError("Expected one bounded coarse ASR window")
                    audio.setpos(round(start * rate))
                    samples = np.frombuffer(audio.readframes(round((end - start) * rate)), dtype="<i2").astype(np.float32) / 32768
                    if rate != 16000:
                        divisor = math.gcd(rate, 16000)
                        samples = resample_poly(samples, 16000 // divisor, rate // divisor)
                    waveform = torch.from_numpy(samples).reshape(1, -1)
                    if args.backend == "transformers":
                        words = portable.align(waveform.reshape(-1).numpy(), row["text"], start, end - start)
                        segments = split_speaker_turns(assign_speakers(word_segments(words, None), turns), None)
                        aligned.extend(segments)
                        continue
                    waveform = torch.nn.functional.layer_norm(waveform, waveform.shape[-1:])
                    normalized = " ".join("".join(c if unicodedata.category(c)[0] in "LMN" else " "
                                                   for c in row["text"].lower()).split())
                    ids = encoder(normalized).to(torch.int64)
                    if decoder(ids) != normalized or not ids.numel() or torch.any(ids == 0):
                        raise ValueError("Tokenizer did not preserve the normalized text")
                    if pipe.tokenizer.vocab_info.unk_idx is not None and torch.any(ids == pipe.tokenizer.vocab_info.unk_idx):
                        raise ValueError("Text contains unsupported symbols")
                    layout = BatchLayout(waveform.shape, seq_lens=[waveform.shape[1]], device=waveform.device)
                    with torch.inference_mode():
                        logits, output = pipe.model(waveform, layout)
                        scores = logits[0, :int(output.seq_lens[0])].float().log_softmax(-1).numpy()
                    spans = align_tokens(scores, ids.tolist())
                    words, group = [], []
                    def flush():
                        if group:
                            words.append(dict(text="".join(t[0] for t in group), start=start + group[0][1] * .02,
                                              end=min(end, start + group[-1][2] * .02), speaker=None))
                            group.clear()
                    for span in spans:
                        piece = decoder(ids[span.token_index:span.token_index + 1])
                        if piece.isspace():
                            flush()
                        else:
                            group.append((piece, span.start_frame, span.end_frame))
                    flush()
                    words = restore_aligned_text(words, row["text"], min_coverage=1.)
                    if join_word_text(words) != row["text"]:
                        raise ValueError("Alignment changed the original text")
                    segments = split_speaker_turns(assign_speakers(word_segments(words, None), turns), None)
                    aligned.extend(segments)
                except (ValueError, RuntimeError) as error:
                    errors.append(dict(window=index, error=str(error)))
                    aligned.append(dict(text=row["text"], start=row["audio_start_time"], end=row["audio_end_time"],
                                        speaker=row.get("speaker_id"), timing="chunk"))
        outcome = dict(id=key, baseline=text_assignment(source["rows"]), candidate=text_assignment(aligned),
                       seconds=time.perf_counter() - started, errors=errors,
                       process_peak_rss_bytes=peak_rss(),
                       source_transcript_sha256=hashlib.sha256((RUNTIME / "results" / key / "transcript.json").read_bytes()).hexdigest(),
                       lexical_policy="Original Qwen text preserved exactly in each aligned window")
        if outcome["baseline"]["recognized_characters"] != outcome["candidate"]["recognized_characters"]:
            raise ValueError("Aligned output changed the total recognized characters")
        (args.output / f"{key}-prediction.json").write_text(json.dumps(aligned, ensure_ascii=False, indent=2))
        (args.output / f"{key}-score.json").write_text(json.dumps(outcome, indent=2))
        print(json.dumps(outcome), flush=True)


if __name__ == "__main__":
    main()
