#!/usr/bin/env python3
"""Capture the production native components using the same synthetic fixture as the web UI."""
import argparse
import os
from pathlib import Path
import subprocess
import shutil
import hashlib
import sys
import tempfile

root = Path(__file__).resolve().parents[1]
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("--output", type=Path, default=Path("/tmp/overtalk-parity"))
parser.add_argument("--fixture", type=Path, default=root / "native/tests/fixtures/visual.json")
parser.add_argument("--binary", type=Path, default=root / "target/debug/examples/visual_review")
parser.add_argument("--views", default="home,meeting,library,settings,notes,speakers,projects,people,context,review,activity")
parser.add_argument("--sizes", default="760x540,1280x720,1920x1080,3840x2160,7680x4320")
parser.add_argument("--light", action="store_true", help="Capture the native light theme")
parser.add_argument("--scale", type=float, default=1.0, help="Operating-system display scale, independently of app zoom")
args = parser.parse_args()
args.output.mkdir(parents=True, exist_ok=True)
with tempfile.TemporaryDirectory(prefix="overtalk-visual-") as temporary:
    # Keep the extension: Windows only starts a program named .exe.
    binary = Path(temporary) / ("visual-review" + args.binary.suffix)
    shutil.copy2(args.binary.resolve(), binary)
    print("Capture binary SHA-256:", hashlib.sha256(binary.read_bytes()).hexdigest(), flush=True)
    engine = root / "native/tests/fixtures/engine.py"
    if os.name == "nt":
        # Windows cannot start a Python script directly; the app starts this instead.
        wrapper = Path(temporary) / "engine.cmd"
        wrapper.write_text(f'@"{sys.executable}" "{engine}" %*\r\n', encoding="utf-8")
        engine = wrapper
    env = dict(os.environ, OVERTALK_SERVICE=str(engine),
               OVERTALK_FIXTURE=str(args.fixture.resolve()), XDG_DATA_HOME=f"{temporary}/data",
               XDG_CONFIG_HOME=f"{temporary}/config", XDG_CACHE_HOME=f"{temporary}/cache",
               OVERTALK_REDUCED_MOTION="1", PYTHONUTF8="1")
    if sys.platform == "darwin":
        # macOS resolves the app's preference folder from HOME, not XDG_*.
        (Path(temporary) / "home").mkdir()
        env["HOME"] = f"{temporary}/home"
    if args.light:
        env["OVERTALK_VISUAL_LIGHT"] = "1"
    env["OVERTALK_VISUAL_SCALE"] = str(args.scale)
    for size in args.sizes.split(","):
        width, height = map(int, size.split("x"))
        views = {"home":0,"meeting":1,"library":2,"settings":3,"notes":8,"speakers":9,"projects":10,"people":11,"context":12,"review":13,"activity":14,"privatemode":15,
                 "connect-picker":16,"connect-linear":17,"connect-jira":18,"connect-google":19,"connect-confluence":20,"connections":21,
                 "guide":22,"guide-review":23,"guide-search":24,"word-tracking":25,"connect-microsoft":26,"connect-slack":27,"connect-notion":28,"connect-github":29,"connect-github-code":30,"send-notes":31,"setup":32,"personal-notes":33,"meeting-ask":34,"followup":35,"recording":36,"meeting-tools":37,"calendar":38,"actions":39,"trash":40,"speech-settings":41,"recording-settings":42,"backup-settings":43,"automation-settings":44,"experimental-settings":45,"vocabulary":46,"reminders":47,"backups":48,"providers":49,"export":50,"task-form":51,"project-form":52,"audio-redaction":53,"reading-options":54,"speaker-name":55,"new-recording":56,"audio-sources":57}
        views.update({"privacy":58,"privacy-policy":59,"privacy-voice":60,"recording-notice":61})
        views.update({"cloud":62,"cloud-funded":63,"cloud-subscriber":63,"cloud-offer":63})
        # Project views use the fixture engine's OVERTALK_VISUAL_PROJECTS answers.
        project_views = {"coming-up":64,"coming-up-recording":65,"brief":66,"brief-facts":67,"suggestion":68,"suggestion-choices":69,
                "matching":70,"projects-automation":71,"projects-general":72,"needs-project":73,"list-options":74,"brief-toast":75,
                "capture-choices":76,"event-choices":77,"project-flows":78}
        views.update(project_views)
        # The recording notice worded for a cloud service, and a meeting's notice record.
        notice = {"recording-notice-external":90,"meeting-notice":91,"meeting-details":92}
        views.update(notice)
        # The first run with both models downloading, the usage report, and remembering a voice.
        views.update({"setup-downloading":93,"usage-report":94,"voice-consent":95})
        # Sharing a copy: a page with source clips, and a redacted Overtalk file.
        views.update({"share-dialog":96,"share-file":97})
        # The About dialog, with the attribution Slint's licence requires.
        views.update({"about":98})
        for name in args.views.split(","):
            if name not in views:
                raise SystemExit(f"Unknown view: {name}")
            view = views[name]
            view_env = dict(env, OVERTALK_VISUAL_SEND="1") if view == 31 else dict(env, OVERTALK_VISUAL_CLOUD="1") if view==63 else env
            if name == "cloud-subscriber":
                view_env = dict(env, OVERTALK_VISUAL_CLOUD="e2e")
            if name == "cloud-offer":
                view_env = dict(env, OVERTALK_VISUAL_CLOUD="e2e-offer")
            if name == "setup-downloading":
                view_env = dict(env, OVERTALK_VISUAL_SETUP="downloading")
            if name in project_views:
                view_env = dict(view_env, OVERTALK_VISUAL_PROJECTS="1")
            if name == "brief-toast":
                view_env["OVERTALK_VISUAL_TOAST"] = "1"
            if name == "project-flows":
                view_env["OVERTALK_VISUAL_FLOWS"] = "1"
            if name in notice:
                view_env = dict(view_env, OVERTALK_VISUAL_NOTICE="external" if name == "recording-notice-external" else "1")
            subprocess.run([str(binary), str(width), str(height), str(view),
                            str(args.output.resolve() / f"slint-{name}-{width}.png")],
                           env=view_env, check=True, timeout=30)
