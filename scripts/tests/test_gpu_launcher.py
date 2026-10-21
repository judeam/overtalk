"""Build entry-point contracts with disposable command stubs, no compiler or GPU."""
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest

REPO = Path(__file__).resolve().parents[2]


@unittest.skipUnless(os.name == "posix", "Unix launcher tests require a POSIX shell")
class GpuLauncherTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="overtalk-build-test-")
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name) / "workspace with spaces"
        frontend = self.root / "frontend"
        (frontend / "scripts").mkdir(parents=True)
        for name in ("build-gpu.sh", "dev-gpu.sh", "scripts/run-gpu.sh", "scripts/tauri-auto.js"):
            shutil.copyfile(REPO / "frontend" / name, frontend / name)
        cli = frontend / "node_modules/@tauri-apps/cli"
        cli.mkdir(parents=True)
        (cli / "tauri.js").write_text("require('fs').writeFileSync(process.env.TEST_LOG, JSON.stringify({args: process.argv.slice(2), cwd: process.cwd(), arch: process.env.CMAKE_CUDA_ARCHITECTURES}));")
        commands = self.root / "commands"
        commands.mkdir()
        node = shutil.which("node")
        scripts = {
            "cargo": '#!/bin/sh\nmkdir -p "$CARGO_TARGET_DIR/release" "$CARGO_TARGET_DIR/debug"\nprintf helper > "$CARGO_TARGET_DIR/release/llama-helper"\nprintf helper > "$CARGO_TARGET_DIR/debug/llama-helper"\nprintf "%s\\n" "$@" > "$TEST_CARGO_LOG"\n',
            "rustc": '#!/bin/sh\nprintf "host: test-triple\\n"\n',
            "pnpm": f'#!/bin/sh\nexec "{node}" scripts/tauri-auto.js "${{2#tauri:}}"\n',
        }
        for name, contents in scripts.items():
            file = commands / name
            file.write_text(contents)
            file.chmod(0o700)
        self.env = {**os.environ, "PATH": f"{commands}:{os.environ['PATH']}",
                    "CARGO_TARGET_DIR": str(self.root / "target"),
                    "TEST_LOG": str(self.root / "tauri.json"),
                    "TEST_CARGO_LOG": str(self.root / "cargo.txt")}
        self.env.pop("CMAKE_CUDA_ARCHITECTURES", None)

    def run_launcher(self, mode, feature):
        return subprocess.run(["bash", str(self.root / "frontend" / f"{mode}-gpu.sh")],
                              cwd="/tmp", env={**self.env, "TAURI_GPU_FEATURE": feature},
                              capture_output=True, text=True, timeout=10)

    def test_modes_features_and_paths(self):
        for mode, feature, helper_feature in [("build", "none", None), ("dev", "cuda", "cuda"),
                                               ("build", "coreml", "metal"), ("dev", "hipblas", None)]:
            with self.subTest(mode=mode, feature=feature):
                result = self.run_launcher(mode, feature)
                self.assertEqual(result.returncode, 0, result.stderr)
                tauri = json.loads(Path(self.env["TEST_LOG"]).read_text())
                self.assertEqual(tauri["args"], [mode] + ([] if feature == "none" else ["--", "--features", feature]))
                self.assertEqual(tauri["cwd"], str(self.root / "frontend"))
                cargo = Path(self.env["TEST_CARGO_LOG"]).read_text().splitlines()
                self.assertEqual("--release" in cargo, mode == "build")
                self.assertEqual("--features" in cargo, helper_feature is not None)
                if helper_feature:
                    self.assertEqual(cargo[cargo.index("--features") + 1], helper_feature)
                if feature == "cuda":
                    self.assertEqual(tauri["arch"], "native")
                self.assertEqual((self.root / "frontend/src-tauri/binaries/llama-helper-test-triple").read_text(), "helper")

    def test_rejects_unrecognized_feature_before_compilation(self):
        result = self.run_launcher("build", "cuda; touch unwanted")
        self.assertEqual(result.returncode, 2)
        self.assertFalse(Path(self.env["TEST_CARGO_LOG"]).exists())

    def test_direct_node_entry_rejects_unrecognized_feature(self):
        result = subprocess.run([shutil.which("node"), str(self.root / "frontend/scripts/tauri-auto.js"), "build"],
                                env={**self.env, "TAURI_GPU_FEATURE": "cuda; touch unwanted"},
                                capture_output=True, timeout=10)
        self.assertEqual(result.returncode, 2)
        self.assertFalse(Path(self.env["TEST_LOG"]).exists())


if __name__ == "__main__":
    unittest.main()
