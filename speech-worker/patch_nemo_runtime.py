"""Apply the reviewed inference-only compatibility patch to NeMo Speech.

Run with the NeMo environment's Python after installing the pinned source below.
All source patterns are validated before writing; changed upstream code fails
closed. Apache-2.0 copyright headers remain intact. No weights are modified.
"""
import importlib.util
from pathlib import Path


def replace_once(text: str, old: str, new: str) -> str:
    if new in text:
        return text
    if text.count(old) != 1:
        raise RuntimeError("NeMo source changed; review the inference patch before installing")
    return text.replace(old, new)


def patch(root: Path) -> None:
    attention = root / "collections/asr/modules/transformer_encoder_utils.py"
    source = attention.read_text()
    source = replace_once(source, "torch.compile(flex_attention, dynamic=True)",
                          "torch.compile(flex_attention, dynamic=False)")
    # Migrate the earlier local test patch as well. All variants use the same
    # bounded tiles, with the normal forward kernel instead of Torch 2.7's
    # decoding specialization (whose Q-length assumption fails on this model).
    source = source.replace("'num_stages': 1}", "'num_stages': 1, 'FORCE_USE_FLEX_ATTENTION': True}")
    marker = "if x.is_cuda and __import__('os').environ.get('OVERTALK_INFERENCE_ONLY') == '1':"
    if marker not in source:
        source = replace_once(source,
            "return _flex_attention_compiled if x.is_cuda else flex_attention",
            marker + "\n        from functools import partial\n"
            "        return partial(_flex_attention_compiled, kernel_options={'BLOCK_M': 32, 'BLOCK_N': 32, 'num_stages': 1, 'FORCE_USE_FLEX_ATTENTION': True})\n"
            "    return _flex_attention_compiled if x.is_cuda else flex_attention")
    exp = root / "utils/exp_manager.py"
    logging = replace_once(exp.read_text(),
        "from lightning.pytorch.loggers import MLFlowLogger, NeptuneLogger, TensorBoardLogger, WandbLogger",
        "from lightning.pytorch.loggers import MLFlowLogger, TensorBoardLogger, WandbLogger\n"
        "from lightning.pytorch.loggers.neptune import NeptuneLogger")
    callback = root / "lightning/callback_group.py"
    callbacks = replace_once(callback.read_text(),
        "from nemo.lightning.one_logger_callback import OneLoggerNeMoCallback",
        "# Overtalk: inference must not initialize training telemetry.\nimport os\n"
        "if os.environ.get('OVERTALK_INFERENCE_ONLY') != '1':\n"
        "    from nemo.lightning.one_logger_callback import OneLoggerNeMoCallback")
    callbacks = replace_once(callbacks,
        "self._callbacks: List[BaseCallback] = [OneLoggerNeMoCallback()]",
        "self._callbacks: List[BaseCallback] = ([] if os.environ.get('OVERTALK_INFERENCE_ONLY') == '1' else [OneLoggerNeMoCallback()])")
    for path, text in [(attention, source), (exp, logging), (callback, callbacks)]:
        if path.read_text() != text:
            path.write_text(text)


if __name__ == "__main__":
    spec = importlib.util.find_spec("nemo")
    if not spec or not spec.submodule_search_locations:
        raise SystemExit("Install the reviewed NeMo Speech runtime first")
    patch(Path(next(iter(spec.submodule_search_locations))))
    print("NeMo inference-only patch verified")
