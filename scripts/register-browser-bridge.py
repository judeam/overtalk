"""Register an explicitly installed local Overtalk browser companion."""
import argparse
import json
import os
from pathlib import Path
import platform
import re

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("--binary", type=Path, required=True)
parser.add_argument("--extension-id", help="Chromium extension ID shown by your browser")
args = parser.parse_args()
binary = args.binary.resolve(strict=True)
home = Path.home()
system = platform.system()
manifest = {"name": "com.overtalk.meetings", "description": "Overtalk local meeting detection", "path": str(binary), "type": "stdio"}
if system == "Darwin":
    roots = [(home / "Library/Application Support/Mozilla/NativeMessagingHosts", "firefox"), (home / "Library/Application Support/Google/Chrome/NativeMessagingHosts", "chrome")]
elif system == "Windows":
    roots = [(Path(os.environ["LOCALAPPDATA"]) / "Overtalk/NativeMessaging/firefox", "firefox"), (Path(os.environ["LOCALAPPDATA"]) / "Overtalk/NativeMessaging/chrome", "chrome")]
else:
    roots = [(home / ".mozilla/native-messaging-hosts", "firefox"), (home / ".config/google-chrome/NativeMessagingHosts", "chrome"), (home / ".config/chromium/NativeMessagingHosts", "chrome")]
for directory, browser in roots:
    if browser == "chrome" and not args.extension_id:
        continue
    value = dict(manifest)
    if browser == "firefox":
        value["allowed_extensions"] = ["meetings@overtalk.local"]
    else:
        if not re.fullmatch("[a-p]{32}", args.extension_id):
            raise SystemExit("Invalid Chromium extension ID")
        value["allowed_origins"] = [f"chrome-extension://{args.extension_id}/"]
    directory.mkdir(parents=True, exist_ok=True)
    path = directory / "com.overtalk.meetings.json"
    path.write_text(json.dumps(value, indent=2))
    if system == "Windows":
        import winreg
        for vendor in (["Mozilla"] if browser == "firefox" else ["Google/Chrome"]):
            key = rf"Software\{vendor.replace('/', chr(92))}\NativeMessagingHosts\com.overtalk.meetings"
            with winreg.CreateKey(winreg.HKEY_CURRENT_USER, key) as handle:
                winreg.SetValueEx(handle, "", 0, winreg.REG_SZ, str(path))
    print(f"Registered {browser} host: {path}")
