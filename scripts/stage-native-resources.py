"""Stage source-controlled runtime resources beside a built native engine.

The engine finds them in Tauri's resource directory: `<prefix>/lib/Sonefold`
for a Linux prefix (the default), the executable's own folder on Windows and
`Overtalk.app/Contents/Resources` on macOS. `--into` stages straight into such
a folder.
"""
import argparse
from pathlib import Path
import shutil

root = Path(__file__).resolve().parents[1]
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("prefix", nargs="?", type=Path, help="Linux install prefix; resources go to <prefix>/lib/Sonefold")
parser.add_argument("--into", type=Path, help="Stage directly into this resource folder")
args = parser.parse_args()
if (args.prefix is None) == (args.into is None):
    parser.error("Give either a prefix or --into")
destination = args.into.resolve() if args.into else args.prefix.resolve() / "lib/Sonefold"
destination.mkdir(parents=True, exist_ok=True)
for source, name in [
    (root / "frontend/src-tauri/templates", "templates"),
    (root / "speech-worker/sonefold_speech", "speech-worker/sonefold_speech"),
    (root / "integrations/sonefold.py", "integrations/sonefold.py"),
    (root / "integrations/browser", "integrations/browser"),
    (root / "scripts/register-browser-bridge.py", "integrations/register-browser-bridge.py"),
]:
    target = destination / name
    target.parent.mkdir(parents=True, exist_ok=True)
    if source.is_dir():
        shutil.copytree(source, target, dirs_exist_ok=True,
                        ignore=shutil.ignore_patterns("__pycache__", "*.pyc"))
    else:
        shutil.copyfile(source, target)
print("Staged native resources:", destination)
