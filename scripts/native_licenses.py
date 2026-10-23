"""Notices every native build ships, on every operating system.

Kept free of platform-only imports so the Windows and macOS packagers can use it.
"""
from pathlib import Path
import shutil
import subprocess

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


def stage_crate_notices(licenses):
    """THIRD-PARTY-RUST.txt: every Rust crate compiled in and the licence text it is used under.

    cargo-about (packaging/licenses) cannot read a LicenseRef text, so Slint's crates are
    listed by hand, pointing at LICENSE.slint.md, which stage_licenses ships."""
    notices = licenses / "THIRD-PARTY-RUST.txt"
    subprocess.run(["cargo", "about", "generate", "--locked", "--fail", "--workspace",
                    "-c", str(ROOT / "packaging/licenses/about.toml"), "-o", str(notices),
                    str(ROOT / "packaging/licenses/about.hbs")], cwd=ROOT, check=True)
    text, crates = notices.read_text(encoding="utf-8").split("@@CRATES@@", 1)
    slint = [f"  {line.split(' | ')[0]}" for line in crates.splitlines() if "LicenseRef-Slint-Royalty-free-2.0" in line]
    if not slint:
        raise SystemExit("cargo-about listed no Slint crates; check packaging/licenses/about.hbs")
    text += ("--------------------------------------------------------------------------------\n"
             "Slint Royalty-free Desktop, Mobile, and Web Applications License 2.0\n"
             "(LicenseRef-Slint-Royalty-free-2.0)\n\nUsed by:\n" + "\n".join(slint) +
             "\n\nThe licence text is LICENSE.slint.md beside this file. The About dialog\n"
             "carries the attribution it asks for.\n")
    notices.write_text(text, encoding="utf-8")
