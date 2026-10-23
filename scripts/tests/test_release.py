"""Release tag checks and notes, against a disposable copy of the version files."""
import importlib.util
import json
from pathlib import Path
import shutil
import tempfile
import unittest

REPO = Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location("release", REPO / "scripts/release.py")
release = importlib.util.module_from_spec(spec)
spec.loader.exec_module(release)

CHANGELOG = """# Changelog

## 0.2.0: second preview (1 December 2026)
- Faster briefs. See [building](docs/BUILDING.md) and [the site](https://overtalk.io/).

## 0.2.0-rc.1 (20 November 2026)
- Candidate.

## 0.1.0: first developer preview (20 October 2026)
- First.
"""


class ReleaseTests(unittest.TestCase):
    def setUp(self):
        folder = tempfile.TemporaryDirectory()
        self.addCleanup(folder.cleanup)
        self.root = Path(folder.name)
        for name in [*release.CARGO_MANIFESTS, *release.JSON_MANIFESTS]:
            (self.root / name).parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(REPO / name, self.root / name)
        self.version = release.versions(REPO)["native/Cargo.toml"]
        current = "" if f"## {self.version}:" in CHANGELOG else f"\n## {self.version}: current\n- Now.\n"
        (self.root / "CHANGELOG.md").write_text(CHANGELOG + current)

    def test_a_tag_must_match_every_manifest_and_have_a_changelog_section(self):
        self.assertEqual(release.check(f"v{self.version}", self.root), [])
        problems = release.check("v9.9.9", self.root)
        self.assertEqual(len(problems), len(release.CARGO_MANIFESTS) + len(release.JSON_MANIFESTS) + 1)
        self.assertIn("CHANGELOG.md has no '## 9.9.9' section", problems)
        self.assertEqual(release.check("0.1.0", self.root), ["0.1.0 is not a version tag such as v0.2.0 or v0.2.0-rc.1"])

    def test_a_changelog_section_stops_at_the_next_version_and_never_matches_a_prefix(self):
        self.assertEqual(release.changelog_section("0.2.0", self.root),
                         "- Faster briefs. See [building](docs/BUILDING.md) and [the site](https://overtalk.io/).")
        self.assertEqual(release.changelog_section("0.2.0-rc.1", self.root), "- Candidate.")
        self.assertIsNone(release.changelog_section("0.2", self.root))

    def test_notes_list_the_downloads_link_into_the_tag_and_say_what_is_unsigned(self):
        with tempfile.TemporaryDirectory() as folder:
            packages = Path(folder)
            for name in ["overtalk_0.2.0_amd64.deb", "Overtalk-0.2.0-setup.exe", "Overtalk-0.2.0-arm64.dmg"]:
                (packages / name).touch()
            (packages / "overtalk-windows-build-info.json").write_text(json.dumps({"signed": False}))
            (packages / "overtalk-macos-arm64-build-info.json").write_text(json.dumps({"signature": "developer-id"}))
            text = release.notes("0.2.0", packages, self.root)
        self.assertIn("[building](https://github.com/judeam/overtalk/blob/v0.2.0/docs/BUILDING.md)", text)
        self.assertIn("[the site](https://overtalk.io/)", text)
        self.assertIn("`.deb`: `overtalk_0.2.0_amd64.deb`", text)
        self.assertNotIn("AppImage", text)
        self.assertIn("SmartScreen", text)
        self.assertNotIn("Open Anyway", text)  # a Developer ID build needs no override


if __name__ == "__main__":
    unittest.main()
