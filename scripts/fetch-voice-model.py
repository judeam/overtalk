#!/usr/bin/env python3
"""Fetch the Apache-2.0 CAM++ ONNX test artifact with a pinned checksum."""
import argparse
import hashlib
from pathlib import Path
import re
import urllib.request

parser=argparse.ArgumentParser(description=__doc__)
parser.add_argument('output',type=Path)
args=parser.parse_args()
source=(Path(__file__).resolve().parents[1]/'crates/voice-memory/src/lib.rs').read_text()
url=re.search(r'pub const MODEL_URL: &str = "([^"]+)"',source)[1]
expected=re.search(r'pub const MODEL_SHA256: &str =\s*"([^"]+)"',source)[1]
args.output.parent.mkdir(parents=True,exist_ok=True)
temporary=args.output.with_suffix('.download')
try:
    digest=hashlib.sha256();size=0
    with urllib.request.urlopen(url,timeout=120) as response,temporary.open('wb') as output:
        while block:=response.read(256*1024):
            size+=len(block)
            if size>32*1024*1024:raise RuntimeError('Model exceeds size limit')
            digest.update(block);output.write(block)
    if digest.hexdigest()!=expected:raise RuntimeError('Model checksum mismatch')
    temporary.replace(args.output)
    print('Verified Apache-2.0 voice model:',size,'bytes')
finally:
    temporary.unlink(missing_ok=True)
