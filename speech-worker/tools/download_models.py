"""Explicit model-only download, separate from the offline analysis process."""

import argparse
import json
from pathlib import Path

REPOS = {
    "cohere-transcribe":"CohereLabs/cohere-transcribe-03-2026",
    "moss-transcribe-diarize":"OpenMOSS-Team/MOSS-Transcribe-Diarize",
    "nemotron3-embed-8b":"nvidia/Nemotron-3-Embed-8B-BF16",
    **{f"qwen3-{kind.lower()}-{size.lower()}": f"Qwen/Qwen3-{kind}-{size}"
       for kind in ("Embedding", "Reranker") for size in ("0.6B", "4B", "8B")},
    "qwen3-asr-1.7b": "Qwen/Qwen3-ASR-1.7B",
    "qwen3-asr-0.6b": "Qwen/Qwen3-ASR-0.6B",
    "qwen3-aligner-0.6b": "Qwen/Qwen3-ForcedAligner-0.6B",
    "parakeet-tdt-0.6b-v3": "nvidia/parakeet-tdt-0.6b-v3",
    "nemotron-3-diarization": "nvidia/Nemotron-3-Diarization",
    "multitalker-parakeet": "nvidia/multitalker-parakeet-streaming-0.6b-v1",
    "nemotron-3.5-streaming": "nvidia/nemotron-3.5-asr-streaming-0.6b",
}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--model", choices=REPOS, required=True)
    parser.add_argument("--directory", type=Path, required=True)
    parser.add_argument("--revision", required=True, help="Reviewed upstream commit SHA or tag (resolved to a SHA)")
    args = parser.parse_args()
    from huggingface_hub import HfApi, snapshot_download
    repo = REPOS[args.model]
    # No recordings/transcripts are read by this utility. Hub auth is used only
    # to download gated weights after upstream terms have been accepted.
    sha = HfApi().model_info(repo_id=repo, revision=args.revision).sha
    if not sha:
        raise RuntimeError("Could not resolve the requested model revision")
    snapshot_download(repo_id=repo, revision=sha, local_dir=str(args.directory),
                      ignore_patterns=["*.gguf", "*.onnx", "*.onnx.data", "*.wav", "*.flac"])
    (args.directory / "sonefold-model.json").write_text(json.dumps({"model": args.model,
        "repo_id": repo, "revision": sha}, indent=2) + "\n", encoding="utf-8")
    print(f"Downloaded {repo} at {sha} to {args.directory.resolve()}")


if __name__ == "__main__":
    main()
