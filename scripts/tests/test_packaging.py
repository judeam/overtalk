"""Platform-neutral checks of the Windows and macOS packagers and the smoke test."""
import importlib.util
import os
from pathlib import Path
import plistlib
import struct
import tempfile
import unittest
import zipfile

ROOT = Path(__file__).resolve().parents[2]


def load(name, path):
    spec = importlib.util.spec_from_file_location(name, ROOT / path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


common = load("packaging_common", "packaging/common.py")
windows = load("packaging_windows", "packaging/windows/build.py")
macos = load("packaging_macos", "packaging/macos/build.py")
service_check = load("check_native_service", "scripts/check-native-service.py")


def portable_executable(imports=(), delayed=()):
    """A minimal PE32+ image whose import tables name the given DLLs."""
    section_rva, raw = 0x1000, 0x200
    body = bytearray(0x400)
    names_at = 0x200
    descriptors = []
    for name in [*imports, *delayed]:
        body[names_at:names_at + len(name) + 1] = name.encode() + b"\0"
        descriptors.append(section_rva + names_at)
        names_at += len(name) + 1
    for index, name_rva in enumerate(descriptors[:len(imports)]):
        struct.pack_into("<IIIII", body, 20 * index, 0, 0, 0, name_rva, 0)
    for index, name_rva in enumerate(descriptors[len(imports):]):
        struct.pack_into("<IIIIIIII", body, 0x100 + 32 * index, 1, name_rva, 0, 0, 0, 0, 0, 0)
    header = bytearray(raw)
    header[:2] = b"MZ"
    struct.pack_into("<I", header, 0x3C, 0x40)
    header[0x40:0x44] = b"PE\0\0"
    struct.pack_into("<HHIIIHH", header, 0x44, 0x8664, 1, 0, 0, 0, 240, 0x22)
    optional = 0x58
    struct.pack_into("<H", header, optional, 0x20B)
    directories = optional + 112
    if imports:
        struct.pack_into("<II", header, directories + 8, section_rva, 20 * (len(imports) + 1))
    if delayed:
        struct.pack_into("<II", header, directories + 8 * 13, section_rva + 0x100, 32 * (len(delayed) + 1))
    section = optional + 240
    header[section:section + 8] = b".rdata\0\0"
    struct.pack_into("<IIII", header, section + 8, len(body), section_rva, len(body), raw)
    return bytes(header) + bytes(body)


class PackagingTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)

    def test_cli_is_found_by_its_binary_name(self):
        metadata = {"packages": [
            {"name": "overtalk-native", "targets": [{"name": "overtalk", "kind": ["bin"]}]},
            {"name": "overtalk-command-line", "targets": [{"name": "overtalk-cli", "kind": ["bin"]}]},
        ]}
        self.assertEqual(common.cli_package(metadata), "overtalk-command-line")
        self.assertIsNone(common.cli_package({"packages": [
            {"name": "x", "targets": [{"name": "overtalk-cli", "kind": ["lib"]}]}]}))

    def test_numeric_versions(self):
        self.assertEqual(common.numeric_version("0.1.0-git20261020.abcdef", 4), "0.1.0.0")
        self.assertEqual(common.numeric_version("1.2.3~git20261020.abc"), "1.2.3")
        self.assertEqual(common.numeric_version("2.0.1-rc.1+engine.abc"), "2.0.1")
        self.assertEqual(common.numeric_version("7"), "7.0.0")

    def test_packaging_builds_keep_flags_whole_and_drop_dynamic_onnx_runtime(self):
        env = common.cargo_env(self.root / "target", ["-C", "target-cpu=x86-64-v3"],
                               base={"ORT_PREFER_DYNAMIC_LINK": "1", "ORT_LIB_LOCATION": str(self.root),
                                     "RUSTFLAGS": "-C target-cpu=native", "PATH": os.environ.get("PATH", "")})
        flags = env["CARGO_ENCODED_RUSTFLAGS"].split("\x1f")
        self.assertEqual(flags[:2], ["-C", "target-cpu=x86-64-v3"])
        self.assertTrue(flags[2].startswith("--remap-path-prefix="))
        for name in ["ORT_PREFER_DYNAMIC_LINK", "ORT_LIB_LOCATION", "RUSTFLAGS"]:
            self.assertNotIn(name, env)
        self.assertEqual(env["TAURI_CONFIG"], '{"bundle":{"externalBin":[]}}')
        self.assertTrue(env["CMAKE_PROJECT_INCLUDE_BEFORE"].endswith("packaging/linux/portable-cpu.cmake"))
        self.assertNotIn("CFLAGS", common.cargo_env(self.root, c_prefix_map=False, base={}))

    def test_zips_are_reproducible(self):
        tree = self.root / "Overtalk"
        (tree / "licenses").mkdir(parents=True)
        (tree / "overtalk.exe").write_bytes(b"MZ")
        (tree / "licenses/LICENSE.md").write_text("MIT")
        first = common.deterministic_zip(tree, self.root / "a.zip", 1760000000, "Overtalk")
        second = common.deterministic_zip(tree, self.root / "b.zip", 1760000000, "Overtalk")
        self.assertEqual(first.read_bytes(), second.read_bytes())
        with zipfile.ZipFile(first) as archive:
            self.assertEqual(archive.namelist(),
                             ["Overtalk/licenses/", "Overtalk/licenses/LICENSE.md", "Overtalk/overtalk.exe"])
            self.assertEqual(archive.getinfo("Overtalk/overtalk.exe").external_attr >> 16, 0o100755)

    def test_pe_imports_include_delay_loaded_dlls(self):
        image = self.root / "engine.exe"
        image.write_bytes(portable_executable(["KERNEL32.dll", "VCRUNTIME140.dll"], ["onnxruntime.dll"]))
        self.assertEqual(windows.pe_imports(image), {"kernel32.dll", "vcruntime140.dll", "onnxruntime.dll"})
        (self.root / "text.exe").write_text("not an image")
        with self.assertRaises(ValueError):
            windows.pe_imports(self.root / "text.exe")

    def test_runtime_dlls_follow_imports_through_the_redistributable(self):
        redist = self.root / "Microsoft.VC143.CRT"
        redist.mkdir()
        (redist / "msvcp140.dll").write_bytes(portable_executable(["vcruntime140.dll", "KERNEL32.dll"]))
        (redist / "vcruntime140.dll").write_bytes(portable_executable(["KERNEL32.dll"]))
        (redist / "concrt140.dll").write_bytes(portable_executable(["KERNEL32.dll"]))
        engine = self.root / "engine.exe"
        engine.write_bytes(portable_executable(["MSVCP140.dll"]))
        self.assertEqual([path.name for path in windows.runtime_dlls([engine], redist)],
                         ["msvcp140.dll", "vcruntime140.dll"])
        self.assertEqual(windows.runtime_dlls([engine], None), [])

    def test_uninstaller_removes_exactly_the_installed_files(self):
        tree = self.root / "Overtalk"
        (tree / "speech-worker/sonefold_speech").mkdir(parents=True)
        (tree / "overtalk.exe").write_bytes(b"MZ")
        (tree / "speech-worker/sonefold_speech/run$1.py").write_text("")
        lists = windows.nsis_file_lists(tree)
        self.assertIn('Delete "$INSTDIR\\speech-worker\\sonefold_speech\\run$$1.py"', lists)
        self.assertIn('Delete "$INSTDIR\\overtalk.exe"', lists)
        uninstall = lists.split("!macro OVERTALK_UNINSTALL_FILES", 1)[1]
        self.assertLess(uninstall.index('RMDir "$INSTDIR\\speech-worker\\sonefold_speech"'),
                        uninstall.index('RMDir "$INSTDIR\\speech-worker"'))
        self.assertNotIn('RMDir "$INSTDIR"', uninstall)

    def signing_environment(self, **values):
        names = ["OVERTALK_WINDOWS_CERT_FILE", "OVERTALK_WINDOWS_CERT_SHA1", "OVERTALK_WINDOWS_CERT_PASSWORD",
                 "OVERTALK_SIGNTOOL", "OVERTALK_TIMESTAMP_URL"]
        saved = {name: os.environ.pop(name, None) for name in names}

        def restore():
            for name, value in saved.items():
                os.environ.pop(name, None)
                if value is not None:
                    os.environ[name] = value
        self.addCleanup(restore)
        os.environ.update(values)

    def test_signing_is_off_without_the_owners_certificate(self):
        self.signing_environment()
        self.assertIsNone(windows.signing_command("Overtalk"))
        self.signing_environment(OVERTALK_WINDOWS_CERT_FILE=r"C:\keys\overtalk.pfx")
        with self.assertRaises(SystemExit):
            windows.signing_command("Overtalk")

    def test_signing_selects_a_thumbprint_and_never_carries_the_password(self):
        password = "hunter2-not-for-logs"
        thumbprint = "0123456789abcdef0123456789ABCDEF01234567"
        self.signing_environment(OVERTALK_WINDOWS_CERT_SHA1=thumbprint, OVERTALK_WINDOWS_CERT_PASSWORD=password,
                                 OVERTALK_SIGNTOOL=r"C:\Kits\signtool.exe")
        command = windows.signing_command("Overtalk")
        self.assertEqual(command[1:4], ["sign", "/sha1", thumbprint])
        self.assertNotIn("/p", command)
        self.assertNotIn("/f", command)
        define = windows.uninstaller_signer(command, self.root)
        wrapper = (self.root / "sign-uninstaller.cmd").read_text()
        self.assertEqual(define, f'"{self.root / "sign-uninstaller.cmd"}" "%1"')
        self.assertIn("/sha1 " + thumbprint, wrapper)
        for text in [" ".join(command), define, wrapper]:
            self.assertNotIn(password, text)
        self.signing_environment(OVERTALK_WINDOWS_CERT_SHA1="not-a-thumbprint")
        with self.assertRaises(SystemExit):
            windows.signing_command("Overtalk")

    def test_echoed_commands_hide_secret_option_values(self):
        import contextlib
        import io
        import sys
        printed = io.StringIO()
        with contextlib.redirect_stdout(printed):
            common.run([sys.executable, "-c", "pass", "/p", "hunter2", "--password", "s3cret", "/P", "x9"])
        for secret in ["hunter2", "s3cret", "x9"]:
            self.assertNotIn(secret, printed.getvalue())
        self.assertEqual(common.printable(["signtool", "sign", "/p", "hunter2", "a.exe"]),
                         "signtool sign /p *** a.exe")

    def test_mac_bundle_declares_its_permissions_and_minimum_system(self):
        info = macos.info_plist("Overtalk", "0.1.0-git20261020.abcdef")
        self.assertEqual(info["LSMinimumSystemVersion"], "14.2")
        self.assertEqual(info["CFBundleShortVersionString"], "0.1.0")
        self.assertEqual(info["CFBundleExecutable"], "overtalk")
        for key in ["NSMicrophoneUsageDescription", "NSAudioCaptureUsageDescription", "NSScreenCaptureUsageDescription"]:
            self.assertIn("Overtalk", info[key])
        plistlib.loads(plistlib.dumps(info))
        self.assertEqual(macos.architecture("aarch64-apple-darwin"), "arm64")
        self.assertEqual(macos.architecture("x86_64-apple-darwin"), "x86_64")
        entitlements = plistlib.loads((ROOT / "packaging/macos/entitlements.plist").read_bytes())
        self.assertEqual(entitlements, {"com.apple.security.device.audio-input": True})

    def test_service_check_isolates_each_systems_profile(self):
        temporary = str(self.root)
        mac = service_check.isolated_environment(temporary, "darwin", {})
        self.assertEqual(mac["HOME"], f"{temporary}/home")
        linux = service_check.isolated_environment(temporary, "linux", {})
        self.assertEqual(linux["XDG_DATA_HOME"], f"{temporary}/data")
        self.assertNotIn("HOME", linux)
        self.assertEqual(service_check.windows_profiles({"APPDATA": r"C:\Users\a\AppData\Roaming"}),
                         [Path(r"C:\Users\a\AppData\Roaming") / "com.judeam.sonefold"])


if __name__ == "__main__":
    unittest.main()
