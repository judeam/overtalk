"""Exercise the generator contract without writing to the real source tree."""
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]


class NativeDispatchTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.script = self.root / "scripts/generate-native-dispatch.py"
        self.script.parent.mkdir()
        shutil.copyfile(ROOT / "scripts/generate-native-dispatch.py", self.script)
        self.source = self.root / "frontend/src-tauri/src"
        self.source.mkdir(parents=True)
        (self.source / "security").mkdir()
        (self.source / "lib.rs").write_text('''
pub async fn save_note(note_id: String) -> Result<(), String> {
    app.emit("note-saved", ());
    Ok(())
}
pub fn note_count() -> u32 { 1 }
pub fn ui_log_diagnostics() {}
tauri::generate_handler![
    save_note,
    note_count,
    ui_log_diagnostics,
]);
''')

    def run_generator(self, *arguments):
        return subprocess.run([sys.executable, str(self.script), *arguments],
                              text=True, capture_output=True, check=False)

    def test_generated_contract_preserves_arguments_unit_results_and_allowlists(self):
        result = self.run_generator()
        self.assertEqual(result.returncode, 0, result.stderr)
        native = (self.source / "native_service/dispatch.rs").read_text()
        web = (self.source / "security/web_dispatch.rs").read_text()
        self.assertIn('argument(args, "noteId")?', native)
        self.assertIn('Ok(Value::Null)', native)
        self.assertIn('"note-saved"', native)
        self.assertIn('pub(super) const COMMANDS', native)
        self.assertIn('pub(super) async fn dispatch', native)
        self.assertIn('pub(super) const EVENTS', native)
        self.assertIn('pub(super) const COMMANDS', web)
        self.assertIn('pub(super) async fn dispatch', web)
        self.assertNotIn('"ui_log_diagnostics"', native)
        self.assertIn('"ui_log_diagnostics"', web)
        self.assertEqual(self.run_generator("--check").returncode, 0)

    def test_check_rejects_stale_output_without_overwriting_it(self):
        self.assertEqual(self.run_generator().returncode, 0)
        target = self.source / "native_service/dispatch.rs"
        stale = target.read_text() + "// outdated\n"
        target.write_text(stale)
        result = self.run_generator("--check")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("Stale adapter", result.stderr)
        self.assertEqual(target.read_text(), stale)

    def test_check_requires_output_and_does_not_create_it(self):
        self.assertNotEqual(self.run_generator("--check").returncode, 0)
        self.assertFalse((self.source / "native_service").exists())


if __name__ == "__main__":
    unittest.main()
