#!/usr/bin/env python3
"""Exercise the actual packager and release guards without contacting GitHub."""
import hashlib
import os
from pathlib import Path
import runpy
import subprocess
import sys
import tempfile
import unittest
import zipfile

ROOT = Path(__file__).resolve().parent.parent
GUARDS = runpy.run_path(str(ROOT / "scripts/verify-release.py"))


class ReleaseTests(unittest.TestCase):
    def setUp(self):
        self.scratch = tempfile.TemporaryDirectory()
        self.addCleanup(self.scratch.cleanup)
        self.root = Path(self.scratch.name)
        self.assets = self.root / "assets"
        self.assets.mkdir()
        for platform in GUARDS["PLATFORMS"]:
            self.make_archive(platform)

    def checksum(self, path):
        digest = hashlib.sha256(path.read_bytes()).hexdigest()
        path.with_suffix(".zip.sha256").write_text(f"{digest}  {path.name}\n", newline="\n")

    def make_archive(self, platform, change=None):
        name = "acsm.exe" if platform.startswith("windows") else "acsm"
        entries = {name: b"test executable", "LICENSE": (ROOT / "LICENSE").read_bytes(),
                   "FONT-LICENSE": (ROOT / "assets/fonts/LICENSE").read_bytes()}
        if change:
            change(entries)
        path = self.assets / f"acsm-{platform}.zip"
        with zipfile.ZipFile(path, "w") as archive:
            for entry, data in entries.items():
                info = zipfile.ZipInfo(entry)
                info.create_system = 3
                info.external_attr = 0o100755 << 16
                archive.writestr(info, data)
        self.checksum(path)
        return path

    def verify(self):
        return GUARDS["verify_archives"](self.assets)

    def test_complete_inventory_and_aggregate(self):
        lines = self.verify().splitlines()
        self.assertEqual(len(lines), 4)
        self.assertEqual(lines, sorted(lines))
        # A prior aggregate from interrupted finalization may be regenerated.
        (self.assets / "SHA256SUMS").write_text("previous aggregate")
        self.assertEqual(self.verify().splitlines(), lines)

    def test_verifier_cli_writes_aggregate_only_after_success(self):
        command = [sys.executable, "-B", "scripts/verify-release.py", "archives", str(self.assets)]
        result = subprocess.run(command, cwd=ROOT, capture_output=True, text=True)
        self.assertEqual(result.returncode, 0, result.stderr)
        aggregate = self.assets / "SHA256SUMS"
        self.assertEqual(aggregate.read_text(), self.verify())
        aggregate.unlink()
        (self.assets / "acsm-linux-x86_64.zip.sha256").write_text("wrong checksum")
        result = subprocess.run(command, cwd=ROOT, capture_output=True, text=True)
        self.assertNotEqual(result.returncode, 0)
        self.assertFalse(aggregate.exists())

    def test_native_packager(self):
        binary = self.root / "fixture binary"
        binary.write_bytes(b"nonempty packaging smoke fixture")
        binary.chmod(0o755)
        prefix = "windows" if os.name == "nt" else "linux"
        for platform in GUARDS["PLATFORMS"]:
            if not platform.startswith(prefix):
                continue
            archive = self.assets / f"acsm-{platform}.zip"
            archive.unlink()
            subprocess.run(["bash", "scripts/package-release.sh", binary.as_posix(), platform,
                            self.assets.as_posix()], cwd=ROOT, check=True)
        self.verify()

    def test_packager_rejects_missing_empty_or_existing_output(self):
        platform = "windows-x86_64" if os.name == "nt" else "linux-x86_64"
        binary = self.root / "fixture binary"
        command = ["bash", "scripts/package-release.sh", binary.as_posix(), platform,
                   self.assets.as_posix()]
        # Missing input, empty input, then an already existing output archive.
        for payload in (None, b"", b"nonempty"):
            if payload is not None:
                binary.write_bytes(payload)
            with self.subTest(payload=payload):
                result = subprocess.run(command, cwd=ROOT, capture_output=True, text=True)
                self.assertNotEqual(result.returncode, 0, result.stdout + result.stderr)
        self.verify()

    def test_missing_archive_or_checksum(self):
        for suffix in (".zip", ".zip.sha256"):
            with self.subTest(suffix=suffix):
                path = self.assets / f"acsm-linux-x86_64{suffix}"
                original = path.read_bytes()
                path.unlink()
                with self.assertRaisesRegex(ValueError, "inventory"):
                    self.verify()
                path.write_bytes(original)

    def test_extra_asset(self):
        (self.assets / "unexpected.zip").write_bytes(b"extra")
        with self.assertRaisesRegex(ValueError, "inventory"):
            self.verify()

    def test_wrong_checksum(self):
        (self.assets / "acsm-linux-x86_64.zip.sha256").write_text("0" * 64)
        with self.assertRaisesRegex(ValueError, "Checksum"):
            self.verify()

    def test_corrupt_archive_with_matching_checksum(self):
        path = self.assets / "acsm-linux-x86_64.zip"
        path.write_bytes(b"not a zip")
        self.checksum(path)
        with self.assertRaises(zipfile.BadZipFile):
            self.verify()

    def test_crc_failure_with_matching_outer_checksum(self):
        path = self.assets / "acsm-linux-x86_64.zip"
        data = bytearray(path.read_bytes())
        data[data.index(b"test executable")] ^= 1
        path.write_bytes(data)
        self.checksum(path)
        with self.assertRaisesRegex(ValueError, "integrity"):
            self.verify()

    def test_wrong_entries_empty_binary_or_license(self):
        changes = [lambda e: e.update({"../escape": b"bad"}),
                   lambda e: e.update({"acsm": b""}),
                   lambda e: e.update({"FONT-LICENSE": b"wrong notice"}),
                   lambda e: e.pop("LICENSE")]
        for change in changes:
            with self.subTest(change=change):
                self.make_archive("linux-x86_64", change)
                with self.assertRaises(ValueError):
                    self.verify()

    def test_missing_executable_permission(self):
        path = self.assets / "acsm-linux-x86_64.zip"
        with zipfile.ZipFile(path) as archive:
            entries = [(info, archive.read(info)) for info in archive.infolist()]
        with zipfile.ZipFile(path, "w") as archive:
            for info, data in entries:
                info.external_attr = 0o100644 << 16
                archive.writestr(info, data)
        self.checksum(path)
        with self.assertRaisesRegex(ValueError, "permission"):
            self.verify()

    def test_tag_manifest_lock_agreement(self):
        (self.root / "Cargo.toml").write_text('[package]\nname = "acsm"\nversion = "1.2.3"\n')
        lock = self.root / "Cargo.lock"
        lock.write_text('[[package]]\nname = "acsm"\nversion = "1.2.3"\n')
        verify = GUARDS["verify_tag"]
        verify("v1.2.3", self.root)
        for tag in ("v1.2.4", "1.2.3", "v1.2.3-rc1"):
            with self.subTest(tag=tag), self.assertRaises(ValueError):
                verify(tag, self.root)
        lock.write_text('[[package]]\nname = "acsm"\nversion = "1.2.2"\n')
        with self.assertRaises(ValueError):
            verify("v1.2.3", self.root)


if __name__ == "__main__":
    unittest.main()
