"""Notices every native build ships, on every operating system.

Kept free of platform-only imports so the Windows and macOS packagers can use it.
"""
from pathlib import Path
import shutil

ROOT = Path(__file__).resolve().parents[1]


def stage_licenses(licenses):
    """Copy the notices every native build ships (installers and packages alike)."""
    licenses.mkdir(parents=True)
    for source in [ROOT / "LICENSE.md", ROOT / "NOTICE.md", ROOT / "native/assets/LICENSE.lucide", ROOT / "native/assets/LICENSE.slint.md", ROOT / "native/assets/LICENSE.scap", ROOT / "native/assets/LICENSE.keyring-MIT", ROOT / "native/assets/LICENSE.keyring-Apache-2.0", ROOT / "frontend/public/fonts/overtalk/OFL-InstrumentSans.txt", ROOT / "frontend/public/fonts/overtalk/OFL-Phudu.txt"]:
        shutil.copy2(source, licenses / source.name)
    shutil.copy2(ROOT / "crates/voice-memory/LICENSE", licenses / "voice-memory-Apache-2.0.txt")
    shutil.copy2(ROOT / "native/assets/LICENSE.connection-logos.txt", licenses / "connection-logos.txt")
    for name in ['glib', 'atty', 'libsqlite3-sys']:
        source = ROOT / 'crates/vendor' / name
        for license_file in source.glob('LICENSE*'):
            shutil.copy2(license_file, licenses / f'{name}-{license_file.name}')
        notice = source / 'OVERTALK-PATCH.md'
        if notice.exists():
            shutil.copy2(notice, licenses / f'{name}-patch.md')
    shutil.copy2(ROOT / "crates/voice-memory/README.md", licenses / "voice-memory-model.md")
