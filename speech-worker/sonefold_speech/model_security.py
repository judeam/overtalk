"""Reject executable model metadata and unsafe sharded checkpoint references.

This is defense in depth for the offline runtime, not a substitute for updating
its ML dependencies or verifying the provenance of a model checkpoint.
"""
import json
from pathlib import Path, PurePosixPath, PureWindowsPath
import re
import tarfile


def require_versions(minimums: dict[str, str]) -> None:
    """Fail closed if an old optional environment would restore known issues."""
    from importlib.metadata import PackageNotFoundError, version
    from packaging.version import Version
    for package, minimum in minimums.items():
        try:
            installed = version(package)
        except PackageNotFoundError as error:
            raise RuntimeError(f"Install the supported speech runtime: missing {package}") from error
        if Version(installed) < Version(minimum):
            raise RuntimeError(f"Update the speech runtime: {package} {minimum} or newer is required for security")


def validate_nemo_checkpoint(path: Path) -> None:
    """Check archive paths and configuration before NeMo extracts/instantiates it."""
    import yaml
    config = None
    total = 0
    with tarfile.open(path) as archive:
        for index, member in enumerate(archive):
            if index >= 10000 or member.issym() or member.islnk() or not (member.isfile() or member.isdir()):
                raise ValueError("Unsafe NeMo archive entry")
            name = PurePosixPath(member.name)
            windows = PureWindowsPath(member.name)
            if name.is_absolute() or windows.is_absolute() or windows.drive or ".." in name.parts or ".." in windows.parts:
                raise ValueError("NeMo archive paths must stay inside the checkpoint")
            total += member.size
            if member.size < 0 or total > 8 * 1024 ** 3:
                raise ValueError("NeMo checkpoint exceeds the extraction limit")
            if name.name == "model_config.yaml":
                if config is not None or member.size > 2 * 1024 * 1024:
                    raise ValueError("Invalid NeMo model configuration")
                with archive.extractfile(member) as stream:
                    config = yaml.safe_load(stream.read(2 * 1024 * 1024 + 1))
    if not isinstance(config, dict):
        raise ValueError("Missing NeMo model configuration")
    pending, seen = [config], set()
    count = 0
    while pending:
        value = pending.pop()
        count += 1
        if count > 50000:
            raise ValueError("NeMo configuration is too complex")
        if isinstance(value, (dict, list)):
            if id(value) in seen:
                continue
            seen.add(id(value))
        if isinstance(value, dict):
            for key, child in value.items():
                if key == "_target_" or (key == "target" and isinstance(child, str) and "." in child):
                    if not isinstance(child, str) or not child.startswith((
                        "nemo.collections.asr.models.", "nemo.collections.asr.modules.",
                        "nemo.collections.asr.losses.", "nemo.collections.common.tokenizers.",
                    )) or not re.fullmatch(r"(?:[A-Za-z_]\w*\.)+[A-Za-z_]\w*", child):
                        raise ValueError("NeMo configuration requests an unsupported executable target")
                pending.append(child)
        elif isinstance(value, list):
            pending.extend(value)
        elif isinstance(value, str) and re.search(r"\$\{[^}]*:", value):
            raise ValueError("NeMo configuration resolvers are not allowed")


def validate_checkpoint(directory: Path) -> None:
    for path in directory.glob("*.json"):
        if path.name not in {"config.json", "processor_config.json", "tokenizer_config.json"} and not path.name.endswith(".index.json"):
            continue
        if not path.is_file() or path.stat().st_size > 16 * 1024 * 1024:
            raise ValueError("Model metadata is missing or exceeds the safe size limit")
        value = json.loads(path.read_text(encoding="utf-8"))
        _metadata(value)
        if path.name.endswith(".index.json"):
            shards = value.get("weight_map", {})
            if not isinstance(shards, dict):
                raise ValueError("Invalid model shard index")
            for shard in set(shards.values()):
                if not isinstance(shard, str):
                    raise ValueError("Invalid model shard reference")
                posix, windows = PurePosixPath(shard), PureWindowsPath(shard)
                if posix.is_absolute() or windows.is_absolute() or windows.drive or ".." in posix.parts or ".." in windows.parts:
                    raise ValueError("Model shard paths must remain inside the checkpoint")
                # HF's cache uses links to content-addressed blobs. A regular
                # target is required; a FIFO/device must never be opened.
                if not (directory / shard).is_file():
                    raise ValueError("Model shard is not a regular file")


def _metadata(value) -> None:
    if isinstance(value, list):
        for child in value:
            _metadata(child)
    elif isinstance(value, dict):
        for key, child in value.items():
            if key == "trust_remote_code" and child:
                raise ValueError("Model metadata cannot enable remote code")
            if key in {"custom_generate", "custom_pipelines"} and child:
                raise ValueError("Executable model extensions are not supported")
            if key in {"_attn_implementation", "_attn_implementation_internal"} and child not in (None, "eager", "sdpa", "flash_attention_2", "flash_attention_3"):
                raise ValueError("Model metadata cannot select an external attention implementation")
            _metadata(child)
