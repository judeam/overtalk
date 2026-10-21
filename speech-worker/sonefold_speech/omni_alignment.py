"""Optional, local-only Omni v1 CTC alignment using the existing Torch runtime.

The original Apache-2.0 Meta checkpoint is mapped into Transformers' equivalent
wav2vec2 modules. No remote code, model download, or ASR text correction occurs.
The fixed tokenizer/checkpoint pair is intentional: v2 has a different vocabulary.
"""
from __future__ import annotations

import hashlib
import math
from pathlib import Path
import unicodedata

from .ctc import align_tokens

REVISION = "8e35f0cc28fa6099e0c14d56db85ce0423baa691"
ARTIFACTS = {
    "omniASR-CTC-300M.pt": "4d31c6a7725a03e90cf26080c67361928755a9f43d322dd574145fbe91c76aec",
    "omniASR_tokenizer.model": "b954cc166b0c9e0271b953fa226fa27ca706a25b7029e84579fe2c60a2b451fe",
}


def normalized_alignment_text(text: str) -> str:
    # The v1 character vocabulary uses decomposed Thai sara am and unvocalized
    # Arabic. Normalize only the alignment target; restore the exact ASR text
    # afterwards. Do not drop unsupported letters or digits to force a match.
    normalized = unicodedata.normalize("NFKC", text).lower()
    normalized = "".join(char for char in normalized
                         if not ("\u064b" <= char <= "\u065f" or char == "\u0670"))
    return " ".join("".join(char if unicodedata.category(char)[0] in "LMN" or char == "'" else " "
                            for char in normalized).split())


def transformer_key(key: str) -> str:
    prefixes = (
        ("encoder_frontend.feature_extractor.layers.", "wav2vec2.feature_extractor.conv_layers."),
        ("encoder_frontend.post_extract_layer_norm.", "wav2vec2.feature_projection.layer_norm."),
        ("encoder_frontend.model_dim_proj.", "wav2vec2.feature_projection.projection."),
        ("encoder_frontend.pos_encoder.conv.", "wav2vec2.encoder.pos_conv_embed.conv."),
        ("encoder.layers.", "wav2vec2.encoder.layers."),
        ("encoder.layer_norm.", "wav2vec2.encoder.layer_norm."),
        ("final_proj.", "lm_head."),
    )
    for before, after in prefixes:
        if key.startswith(before):
            key = after + key[len(before):]
            return (key.replace("weight_g", "parametrizations.weight.original0")
                    .replace("weight_v", "parametrizations.weight.original1")
                    .replace(".self_attn_layer_norm.", ".layer_norm.")
                    .replace(".ffn_layer_norm.", ".final_layer_norm.")
                    .replace(".self_attn.output_proj.", ".attention.out_proj.")
                    .replace(".self_attn.", ".attention.")
                    .replace(".ffn.inner_proj.", ".feed_forward.intermediate_dense.")
                    .replace(".ffn.output_proj.", ".feed_forward.output_dense."))
    raise ValueError(f"Unsupported Omni checkpoint tensor: {key}")


def unspaced_character(piece: str) -> bool:
    """Keep CJK/Thai timings fine-grained, including code-switched passages."""
    return any(0x0E00 <= ord(char) <= 0x0E7F or 0x3040 <= ord(char) <= 0x30FF
               or 0x3400 <= ord(char) <= 0x9FFF or 0x20000 <= ord(char) <= 0x323AF
               for char in piece)


class OmniAlignment:
    def __init__(self, directory: Path):
        for filename, expected in ARTIFACTS.items():
            with (directory / filename).open("rb") as stream:
                actual = hashlib.file_digest(stream, "sha256").hexdigest()
            if actual != expected:
                raise ValueError(f"Omni alignment checksum mismatch: {filename}")
        import torch
        import sentencepiece
        from transformers import Wav2Vec2Config, Wav2Vec2ForCTC

        self.torch = torch
        self.tokenizer = sentencepiece.SentencePieceProcessor(model_file=str(directory / "omniASR_tokenizer.model"))
        if self.tokenizer.vocab_size() != 9812 or self.tokenizer.unk_id() != 3:
            raise ValueError("Unexpected Omni v1 character vocabulary")
        config = Wav2Vec2Config(
            vocab_size=9812, hidden_size=1024, num_hidden_layers=24,
            num_attention_heads=16, intermediate_size=4096, conv_bias=True,
            feat_extract_norm="layer", do_stable_layer_norm=True,
            hidden_dropout=0, activation_dropout=0, attention_dropout=0,
            feat_proj_dropout=0, final_dropout=0, mask_time_prob=0,
            mask_feature_prob=0, apply_spec_augment=False,
        )
        # Meta initialization avoids allocating a second 1.3 GB set of weights.
        with torch.device("meta"):
            model = Wav2Vec2ForCTC(config)
        state = torch.load(directory / "omniASR-CTC-300M.pt", map_location="cpu", weights_only=True, mmap=True)["model"]
        converted = {transformer_key(key): value for key, value in state.items()}
        if len(converted) != len(state):
            raise ValueError("Duplicate Omni checkpoint tensor mapping")
        model.load_state_dict(converted, strict=True, assign=True)
        self.model = model.eval()

    def align(self, samples, text: str, offset: float, duration: float,
              language: str | None = None) -> list[dict]:
        from .asr import join_word_text, restore_aligned_text

        if not math.isfinite(offset) or offset < 0 or not 0 < duration <= 30.001:
            raise ValueError("Omni alignment requires a bounded 30-second audio window")
        if len(samples) > 480016 or len(samples) < 400:
            raise ValueError("Invalid 16 kHz alignment window")
        normalized = normalized_alignment_text(text)
        ids = self.tokenizer.encode(normalized, out_type=int)
        if (not ids or any(token in {0, 1, 2, 3} for token in ids)
                or self.tokenizer.decode(ids) != normalized):
            raise ValueError("Omni tokenizer cannot preserve the alignment text")
        torch = self.torch
        waveform = torch.as_tensor(samples, dtype=torch.float32).reshape(1, -1)
        if not torch.isfinite(waveform).all():
            raise ValueError("Non-finite alignment audio")
        waveform = torch.nn.functional.layer_norm(waveform, waveform.shape[-1:])
        with torch.inference_mode():
            scores = self.model(waveform).logits[0].float().log_softmax(-1).numpy()
        spans = align_tokens(scores, ids)
        words, group = [], []

        def flush():
            if group:
                start = offset + group[0][1] * .02
                end = min(offset + duration, offset + group[-1][2] * .02)
                if end <= start:
                    raise ValueError("Alignment produced an invalid word duration")
                words.append(dict(text="".join(part[0] for part in group), start=start, end=end,
                                  speaker=None, alignment_score=sum(part[3] for part in group) / len(group)))
                group.clear()

        for span in spans:
            piece = self.tokenizer.decode([ids[span.token_index]])
            if piece.isspace():
                flush()
            else:
                separate = unspaced_character(piece)
                if separate:
                    flush()
                group.append((piece, span.start_frame, span.end_frame, span.mean_log_probability))
                if separate:
                    flush()
        flush()
        words = restore_aligned_text(words, text, min_coverage=1.)
        if join_word_text(words, language) != text:
            raise ValueError("Alignment failed to preserve original ASR text")
        return words
