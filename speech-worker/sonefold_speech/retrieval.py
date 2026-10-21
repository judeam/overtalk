"""Offline Qwen retrieval, correct last-token pooling and yes/no reranking.

No remote Python is trusted. One model is resident at a time. Query instructions
and reranker framing follow the official Qwen3 model cards; responses contain
numbers only. Native code owns filtering, source revisions and persistence.
"""
from __future__ import annotations
import gc
import json
import math
from pathlib import Path
from typing import Any
from .asr import SpeechError, choose_device

TASK = "Retrieve meeting passages that contain evidence answering the question"
SIZES = {"0.6B", "4B", "8B"}


def last_token_pool(hidden, mask):
    # Last attended position works for both left and right padding.
    import torch
    positions = torch.arange(mask.shape[1], device=mask.device).expand_as(mask)
    last = positions.masked_fill(mask == 0, -1).max(dim=1).values
    if (last < 0).any():
        raise SpeechError("Embedding input has no tokens")
    return hidden[torch.arange(hidden.shape[0], device=hidden.device), last]


def local_checkpoint(kind: str, size: str, directory: str | None, family="qwen3"):
    if kind not in {"Embedding", "Reranker"} or size not in SIZES:
        raise SpeechError("Unsupported retrieval model")
    repo = "nvidia/Nemotron-3-Embed-8B-BF16" if family=="nemotron3" and kind=="Embedding" else f"Qwen/Qwen3-{kind}-{size}"
    if family=="nemotron3" and kind=="Embedding" and size!="8B":
        raise SpeechError("Nemotron 3 Embed quality profile uses 8B")
    if directory:
        path = Path(directory).expanduser().resolve()
        if not path.is_dir():
            raise SpeechError("Retrieval model directory does not exist")
        metadata = path / "sonefold-model.json"
        if not metadata.is_file():
            raise SpeechError("Use the pinned model download utility to record the retrieval revision")
        info = json.loads(metadata.read_text())
        if info.get("repo_id") != repo:
            raise SpeechError("The retrieval directory contains a different model")
        revision = info.get("revision", "")
    else:
        from huggingface_hub import snapshot_download
        # Resolve only locally cached snapshots. No inference-time networking.
        path = Path(snapshot_download(repo, local_files_only=True)).resolve()
        revision = path.name
    if len(revision) != 40 or any(c not in "0123456789abcdef" for c in revision):
        raise SpeechError("Retrieval model needs a recorded commit SHA")
    from .model_security import validate_checkpoint
    validate_checkpoint(path)
    return path, f"{repo}@{revision}"


class RetrievalEngine:
    def __init__(self):
        self.model = self.tokenizer = self.identity = None
        self.torch = None

    def unload(self):
        self.model = self.tokenizer = self.identity = None
        gc.collect()
        if self.torch is not None and self.torch.cuda.is_available():
            self.torch.cuda.empty_cache()

    def load(self, kind, size, directory, requested_device, family):
        import torch
        from transformers import AutoModel, AutoModelForCausalLM, AutoTokenizer
        path, revision = local_checkpoint(kind, size, directory, family)
        device, dtype = choose_device(torch, requested_device)
        identity = (revision, device)
        if identity != self.identity:
            self.unload()
            self.torch = torch
            self.tokenizer = AutoTokenizer.from_pretrained(str(path), local_files_only=True,
                                                           trust_remote_code=False, padding_side="left")
            cls = AutoModel if kind == "Embedding" else AutoModelForCausalLM
            self.model = cls.from_pretrained(str(path), local_files_only=True,
                                            trust_remote_code=False, torch_dtype=dtype,
                                            use_safetensors=True,
                                            attn_implementation="sdpa").to(device).eval()
            self.identity = identity
        return revision

    def handle(self, request: dict[str, Any]):
        operation = request["operation"]
        kind = "Reranker" if operation == "knowledge_rerank" else "Embedding"
        family = request.get("family", "nemotron3") if kind=="Embedding" else "qwen3"
        size = request.get("size", "8B")
        directory = request.get("directory") or None
        if operation == "knowledge_health":
            result = {}
            for k in ("Embedding", "Reranker"):
                try:
                    _, revision = local_checkpoint(k, request.get("reranker_size", "4B") if k=="Reranker" else size, request.get(k.lower() + "_dir"), "qwen3" if k=="Reranker" else family)
                    result[k.lower()] = {"cached": True, "revision": revision}
                except (ImportError, OSError, ValueError, SpeechError) as error:
                    result[k.lower()] = {"cached": False, "diagnostic": str(error)}
            return result
        documents = request.get("texts")
        if not isinstance(documents, list) or not 1 <= len(documents) <= 16 or any(
            not isinstance(t, str) or not t.strip() or len(t) > 6000 for t in documents
        ):
            raise SpeechError("Provide 1–16 nonempty retrieval texts below 6,000 characters")
        revision = self.load(kind, size, directory, request.get("device", "auto"), family)
        torch = self.torch
        query = request.get("query")
        if operation == "knowledge_embed":
            if family=="nemotron3":
                documents=[("query: " if query is True else "passage: ")+text for text in documents]
            elif query is True:
                documents = [f"Instruct: {TASK}\nQuery: {text}" for text in documents]
            vectors=[]
            for doc in documents:
                inputs=self.tokenizer([doc],return_tensors="pt",padding=True,truncation=True,max_length=4096).to(self.model.device)
                with torch.inference_mode():
                    hidden=self.model(**inputs).last_hidden_state
                    if family=="nemotron3":
                        mask=inputs.attention_mask.unsqueeze(-1).to(hidden.dtype)
                        pooled=(hidden*mask).sum(dim=1)/mask.sum(dim=1).clamp(min=1)
                    else:pooled=last_token_pool(hidden,inputs.attention_mask)
                    vectors.extend(torch.nn.functional.normalize(pooled,p=2,dim=1).float().cpu().tolist())
            return {"revision": revision, "vectors": vectors}
        if operation != "knowledge_rerank" or not isinstance(query, str) or len(query) > 2000:
            raise SpeechError("Invalid reranking request")
        # Reserve the instruction and the assistant suffix before truncation.
        prefix = '<|im_start|>system\nJudge whether the Document meets the requirements based on the Query and the Instruct provided. Note that the answer can only be "yes" or "no".<|im_end|>\n<|im_start|>user\n'
        suffix = '<|im_end|>\n<|im_start|>assistant\n<think>\n\n</think>\n\n'
        pre = self.tokenizer.encode(prefix, add_special_tokens=False)
        post = self.tokenizer.encode(suffix, add_special_tokens=False)
        scores = []
        # Small batches bound 4B model logits/activation memory on desktop GPUs.
        for offset in range(0, len(documents), 2):
            texts = [f"<Instruct>: {TASK}\n<Query>: {query}\n<Document>: {text}" for text in documents[offset:offset + 2]]
            tokens = self.tokenizer(texts, padding=False, truncation=True,
                                    max_length=4096-len(pre)-len(post), add_special_tokens=False)
            inputs = self.tokenizer.pad({"input_ids": [pre + ids + post for ids in tokens.input_ids]},
                                         padding=True, return_tensors="pt").to(self.model.device)
            with torch.inference_mode():
                logits = self.model(**inputs, logits_to_keep=1).logits[:, -1, :].float()
                pair = torch.stack([logits[:, self.tokenizer.convert_tokens_to_ids("no")],
                                    logits[:, self.tokenizer.convert_tokens_to_ids("yes")]], dim=1)
                scores.extend(torch.softmax(pair, dim=1)[:, 1].cpu().tolist())
        if not all(math.isfinite(s) for s in scores):
            raise SpeechError("Reranker returned non-finite scores")
        return {"revision": revision, "scores": scores}
