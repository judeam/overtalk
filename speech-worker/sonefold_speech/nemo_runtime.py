"""Initialize the optional NeMo inference runtime without training telemetry."""
import os


def prepare() -> None:
    from .model_security import require_versions
    require_versions({"torch": "2.13.0", "transformers": "5.17.0", "hydra-core": "1.3.7",
                      "lightning": "2.6.6", "pytorch-lightning": "2.6.6", "fsspec": "2026.6.0"})
    # Explicitly enforce PyTorch's restricted unpickler even if the parent
    # process exported the legacy compatibility escape hatch.
    os.environ.pop("TORCH_FORCE_NO_WEIGHTS_ONLY_LOAD", None)
    os.environ["TORCH_FORCE_WEIGHTS_ONLY_LOAD"] = "1"
    os.environ["OVERTALK_INFERENCE_ONLY"] = "1"
    os.environ["HF_HUB_OFFLINE"] = "1"
    os.environ["TRANSFORMERS_OFFLINE"] = "1"
