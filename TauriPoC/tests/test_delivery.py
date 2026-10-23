import hashlib
import importlib.util
from pathlib import Path
import json
import tarfile
import tempfile
import unittest
import zipfile

SCRIPT = Path(__file__).resolve().parents[1] / "scripts/payload.py"
spec = importlib.util.spec_from_file_location("payload", SCRIPT)
payload = importlib.util.module_from_spec(spec)
spec.loader.exec_module(payload)

class DeliveryTests(unittest.TestCase):
    def test_archive_sha256_precedes_extraction(self):
        with tempfile.TemporaryDirectory() as directory:
            cache = Path(directory)
            (cache / "archive.zip").write_bytes(b"altered archive")
            with self.assertRaisesRegex(ValueError, "integrity failed"):
                payload.download({"filename": "archive.zip", "sha256": "0" * 64, "url": "https://example.invalid/never-requested"}, cache)

    def test_zip_rejects_traversal_and_duplicates(self):
        for names in [("../escape",), ("C:escape",), ("/escape",), ("a\\escape",), ("A", "a"), ("NUL",), ("file. ",), ("COM1.txt",)]:
            with self.subTest(names=names), tempfile.TemporaryDirectory() as directory:
                archive = Path(directory) / "archive.zip"
                with zipfile.ZipFile(archive, "w") as output:
                    for name in names:
                        output.writestr(name, "untrusted")
                with self.assertRaises(ValueError):
                    payload.extract(archive, Path(directory) / "result")
                self.assertFalse((Path(directory) / "escape").exists())

    def test_tar_rejects_escaping_symlink_and_hardlink(self):
        for kind, link in [(tarfile.SYMTYPE, "../../escape"), (tarfile.LNKTYPE, "tool")]:
            with tempfile.TemporaryDirectory() as directory:
                archive = Path(directory) / "archive.tar.gz"
                with tarfile.open(archive, "w:gz") as output:
                    entry = tarfile.TarInfo("root/link")
                    entry.type = kind
                    entry.linkname = link
                    output.addfile(entry)
                with self.assertRaises(ValueError):
                    payload.extract(archive, Path(directory) / "result")

    def test_zip_preserves_payload_and_executable_mode(self):
        with tempfile.TemporaryDirectory() as directory:
            archive = Path(directory) / "archive.zip"
            with zipfile.ZipFile(archive, "w") as output:
                entry = zipfile.ZipInfo("tools/adb")
                entry.external_attr = 0o100755 << 16
                output.writestr(entry, b"actual executable bytes")
            root = Path(directory) / "result"
            payload.extract(archive, root)
            entries = payload.inventory(root)
            self.assertEqual(entries[0]["sha256"], hashlib.sha256(b"actual executable bytes").hexdigest())
            self.assertEqual(entries[0]["path"], "tools/adb")

    def test_pins_have_only_complete_supported_targets(self):
        lock = json.loads((SCRIPT.parents[1] / "dependencies/downloads.lock.json").read_text())
        self.assertEqual(set(lock["targets"]), payload.TARGETS)
        for records in lock["targets"].values():
            self.assertEqual({record["name"] for record in records}, {"platform-tools", "mitmproxy"})
            for record in records:
                self.assertEqual(len(record["sha256"]), 64)
                self.assertTrue(record["url"].startswith("https://"))

if __name__ == "__main__":
    unittest.main()
