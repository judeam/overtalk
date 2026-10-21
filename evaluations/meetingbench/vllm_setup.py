"""Local vLLM setup matching Qwen's corrected Transformers tokenizer."""
import hashlib
import json
import os
from pathlib import Path
import tempfile


def tokenizer_directory(checkpoint, cache_root, factory=None):
    checkpoint, cache_root=Path(checkpoint),Path(cache_root)
    source_files=sorted(p for p in checkpoint.iterdir() if p.suffix in {'.json','.txt'} and p.name!='README.md')
    fingerprint=hashlib.sha256()
    for path in source_files:
        fingerprint.update(path.name.encode());fingerprint.update(path.read_bytes())
    fingerprint.update(b'fix_mistral_regex=True-v1')
    target=cache_root/fingerprint.hexdigest()
    if (target/'tokenizer_config.json').is_file():
        if not json.loads((target/'tokenizer_config.json').read_text()).get('fix_mistral_regex'):
            raise ValueError('Cached vLLM tokenizer does not retain the Qwen correction')
        return target
    if factory is None:
        from transformers import AutoTokenizer
        factory=AutoTokenizer.from_pretrained
    tokenizer=factory(str(checkpoint),local_files_only=True,fix_mistral_regex=True)
    cache_root.mkdir(parents=True,exist_ok=True)
    with tempfile.TemporaryDirectory(dir=cache_root,prefix='preparing-') as temporary:
        temporary=Path(temporary)
        tokenizer.save_pretrained(temporary)
        cfg=temporary/'tokenizer_config.json'
        data=json.loads(cfg.read_text());data['fix_mistral_regex']=True
        cfg.write_text(json.dumps(data,ensure_ascii=False,indent=2)+'\n')
        try:temporary.rename(target)
        except FileExistsError:pass
    return target


def local_engine_options(checkpoint, *, eager=False):
    os.environ.setdefault('VLLM_WORKER_MULTIPROC_METHOD','spawn')
    os.environ['VLLM_NO_USAGE_STATS']='1'
    os.environ['DO_NOT_TRACK']='1'
    os.environ['VLLM_HOST_IP']='127.0.0.1'
    cache=Path(__file__).resolve().parent/'.runtime/tokenizers'
    view=tokenizer_directory(checkpoint,cache)
    # Some vLLM processor paths reload the model's tokenizer directly and ignore
    # tokenizer overrides. Expose the same immutable weights through a private
    # view whose tokenizer configuration always retains Qwen's correction.
    for source in Path(checkpoint).iterdir():
        target=view/source.name
        if source.is_file() and not target.exists():
            target.symlink_to(source.resolve())
    options=dict(model=str(view),tokenizer=str(view),dtype='bfloat16')
    if not eager:
        # Full decode-graph capture stalled on this capped MPS workload.
        # Piecewise graphs retain compiled decoder kernels without that capture.
        options['compilation_config']={'cudagraph_mode':'PIECEWISE'}
    return options
