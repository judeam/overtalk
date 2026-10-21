"""Install the pinned official Slint LSP without changing global tools."""
import hashlib
import io
import json
import os
from pathlib import Path
import platform
import tarfile
import urllib.request
import zipfile

VERSION = "1.18.1"
machine = platform.machine().lower()
system = platform.system()
if system == "Windows":
    target = "aarch64-pc-windows-msvc" if machine in {"arm64", "aarch64"} else "x86_64-pc-windows-msvc"
    extension, executable = "zip", "slint-lsp.exe"
elif system == "Darwin":
    target, extension, executable = "universal-apple-darwin", "tar.gz", "slint-lsp"
elif system == "Linux":
    target = "aarch64-unknown-linux-gnu" if machine in {"arm64", "aarch64"} else "x86_64-unknown-linux-gnu"
    extension, executable = "tar.gz", "slint-lsp"
else:
    raise SystemExit(f"Unsupported system: {system}")
name = f"slint-lsp-{target}.{extension}"
headers = {"User-Agent": "Overtalk-native-tool-setup", "Accept": "application/vnd.github+json"}
# Shared CI egress IPs exhaust GitHub's unauthenticated quota. This credential
# is sent only to the GitHub metadata endpoint, never to the asset/CDN download.
token = os.environ.get("GITHUB_TOKEN") or os.environ.get("GH_TOKEN")
if token:
    headers["Authorization"] = f"Bearer {token}"
request = urllib.request.Request(
    f"https://api.github.com/repos/slint-ui/slint/releases/tags/v{VERSION}",
    headers=headers,
)
class NoRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, request, fp, code, msg, headers, newurl):
        return None

# An Authorization header on urllib requests is otherwise copied to redirected
# hosts. The fixed metadata URL must respond directly; never forward the token.
metadata_client = urllib.request.build_opener(urllib.request.ProxyHandler({}), NoRedirect())
with metadata_client.open(request, timeout=30) as response:
    data = response.read(2 * 1024 * 1024 + 1)
    if len(data) > 2 * 1024 * 1024:
        raise SystemExit("Release metadata exceeds the size limit")
    release = json.loads(data)
asset = next(item for item in release["assets"] if item["name"] == name)
expected = asset.get("digest", "")
if not expected.startswith("sha256:"):
    raise SystemExit("Release did not provide a SHA-256 digest; refusing an unverified executable")
with urllib.request.urlopen(asset["browser_download_url"], timeout=60) as response:
    data = response.read(100 * 1024 * 1024 + 1)
if "sha256:" + hashlib.sha256(data).hexdigest() != expected:
    raise SystemExit("Downloaded LSP did not match the release checksum")
if extension == "zip":
    with zipfile.ZipFile(io.BytesIO(data)) as archive:
        member = next(item for item in archive.namelist() if Path(item).name == executable)
        binary = archive.read(member)
else:
    with tarfile.open(fileobj=io.BytesIO(data)) as archive:
        member = next(item for item in archive.getmembers() if item.isfile() and Path(item.name).name == executable)
        binary = archive.extractfile(member).read()
destination = Path(__file__).resolve().parents[1] / ".tools/slint/bin" / executable
destination.parent.mkdir(parents=True, exist_ok=True)
destination.write_bytes(binary)
destination.chmod(0o755)
settings_path = destination.parents[3] / ".vscode/settings.json"
settings_path.parent.mkdir(exist_ok=True)
settings = json.loads(settings_path.read_text(encoding="utf-8")) if settings_path.exists() else {}
settings["slint.lspBinaryPath"] = str(destination)
settings["[slint]"] = {"editor.defaultFormatter": "slint.slint", "editor.formatOnSave": True}
settings["rust-analyzer.check.command"] = "clippy"
settings_path.write_text(json.dumps(settings, indent=2) + "\n", encoding="utf-8")
print(f"Installed Slint LSP {VERSION}: {destination}")
