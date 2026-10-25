import hashlib
import json
import os
import plistlib
import tarfile
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch
from types import SimpleNamespace

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "scripts"))
import android_release as updates


class SimulatedCrash(BaseException):
    pass


class FakeGitHub:
    def __init__(self, previous=None, crash_at=None, fail_promotion=False):
        self.releases = {}
        if previous is not None:
            self.releases[updates.FEED_TAG] = {"isDraft":False, "assets":{"android-updater.json":json.dumps(previous).encode()}}
        self.crash_at = crash_at
        self.fail_promotion = fail_promotion
        self.corrupt_download = False
        self.mutations = 0

    def checkpoint(self):
        self.mutations += 1
        if self.mutations == self.crash_at:
            raise SimulatedCrash()

    def run(self, command, **kwargs):
        assert command[:2] == ["gh", "release"]
        arguments = command[2:]
        repository = arguments.index("--repo")
        arguments = arguments[:repository] + arguments[repository + 2:]
        action = arguments[0]
        if action == "list":
            return SimpleNamespace(stdout=json.dumps([{"tagName":tag, "isDraft":release["isDraft"]} for tag, release in self.releases.items()]))
        tag = arguments[1]
        if action == "view":
            return SimpleNamespace(stdout=json.dumps({"assets":[{"name":name} for name in self.releases[tag]["assets"]]}))
        if action == "create":
            assert tag not in self.releases
            self.releases[tag] = {"isDraft":"--draft" in arguments, "assets":{}}
            self.checkpoint()
        elif action == "edit":
            self.releases[tag]["isDraft"] = False
            self.checkpoint()
        elif action == "upload":
            for filename in arguments[2:]:
                if filename == "--clobber":
                    continue
                path = Path(filename)
                name = path.name
                if "--clobber" in arguments:
                    self.releases[tag]["assets"].pop(name, None)
                    self.checkpoint()
                else:
                    assert name not in self.releases[tag]["assets"]
                if tag == updates.FEED_TAG and name == "android-updater.json" and self.fail_promotion:
                    self.fail_promotion -= 1
                    raise subprocess.CalledProcessError(1, command)
                self.releases[tag]["assets"][name] = path.read_bytes()
                self.checkpoint()
        elif action == "download":
            name = arguments[arguments.index("--pattern") + 1]
            directory = Path(arguments[arguments.index("--dir") + 1])
            directory.mkdir(parents=True, exist_ok=True)
            content = self.releases[tag]["assets"][name]
            if self.corrupt_download and tag.startswith("android-v"):
                content += b"modified"
            (directory / name).write_bytes(content)
        else:
            raise AssertionError(action)
        return SimpleNamespace(stdout="")


class UpdateDeliveryTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.temporary = tempfile.TemporaryDirectory()
        cls.directory = Path(cls.temporary.name)
        cli = ROOT / "node_modules/@tauri-apps/cli/tauri.js"
        cls.key = cls.directory / "fixture.key"
        cls.other_key = cls.directory / "other.key"
        for key in (cls.key, cls.other_key):
            subprocess.run(["node", str(cli), "signer", "generate", "--ci", "--password", "fixture-only", "--write-keys", str(key)], cwd=ROOT, check=True, stdout=subprocess.DEVNULL)
        cls.public = Path(str(cls.key) + ".pub").read_text().strip()
        cls.environment = {"MOBILE_CAPTURE_UPDATER_PUBLIC_KEY": cls.public, "TAURI_SIGNING_PRIVATE_KEY_PATH": str(cls.key), "TAURI_SIGNING_PRIVATE_KEY_PASSWORD": "fixture-only"}
        cls.artifact = cls.directory / "final.app.tar.gz"
        app = cls.directory / "Mobile Capture Android.app"
        payload = app / "Contents/Resources/android-tools"
        payload.mkdir(parents=True)
        (payload / "payload.json").write_text('{"fixture": true}')
        (app / "Contents/Info.plist").write_bytes(plistlib.dumps({"CFBundleShortVersionString":"0.2.0"}))
        with tarfile.open(cls.artifact, "w:gz") as archive:
            archive.add(app, arcname=app.name)
        with patch.dict(os.environ, cls.environment):
            cls.signature = updates.sign(cls.artifact, "0.2.0")

    @classmethod
    def tearDownClass(cls):
        cls.temporary.cleanup()

    def test_real_signature_rejects_modified_bytes_wrong_key_and_wrong_version(self):
        signature = Path(str(self.artifact) + ".sig")
        updates.verify(self.artifact, signature, self.public, "0.2.0")
        for public, version in [(self.public, "0.3.0"), (Path(str(self.other_key) + ".pub").read_text().strip(), "0.2.0")]:
            with self.assertRaises(subprocess.CalledProcessError):
                updates.verify(self.artifact, signature, public, version)
        altered = self.directory / "altered.app.tar.gz"
        altered.write_bytes(self.artifact.read_bytes() + b"post-signing modification")
        with self.assertRaises(subprocess.CalledProcessError):
            updates.verify(altered, signature, self.public, "0.2.0")

    def packages(self, root):
        packages = {}
        for target in updates.TARGETS:
            directory = root / target
            directory.mkdir()
            name = "MobileCaptureAndroid-0.2.0-" + ("Windows-x64.exe" if target.startswith("x86") else "macOS-arm64.app.tar.gz")
            artifact = directory / name
            artifact.write_bytes(self.artifact.read_bytes())
            Path(str(artifact) + ".sig").write_text(self.signature)
            build = {"version":"0.2.0", "target":target, "channel":"release", "sourceDirty":False, "commit":"fixture-commit", "updaterPublicKeySHA256":hashlib.sha256(self.public.encode()).hexdigest()}
            (directory / "BUILD.json").write_text(json.dumps(build))
            (directory / "UPDATER.json").write_text(json.dumps({"version":"0.2.0", "target":target, "artifact":name, "sha256":updates.sha256(artifact), "signature":self.signature}))
            (directory / "SHA256SUMS").write_text("".join(updates.sha256(path) + "  " + path.name + "\n" for path in sorted(directory.iterdir()) if path.is_file()))
            packages[target] = directory
        return packages

    def test_manifest_has_both_verified_targets_and_versioned_android_urls(self):
        with tempfile.TemporaryDirectory() as directory:
            packages = self.packages(Path(directory))
            manifest = updates.manifest(packages, "0.2.0", self.public, "Release notes")
            self.assertEqual(set(manifest["platforms"]), {"darwin-aarch64", "windows-x86_64"})
            for entry in manifest["platforms"].values():
                self.assertIn("/android-v0.2.0/MobileCaptureAndroid-0.2.0-", entry["url"])
                self.assertEqual(entry["signature"], self.signature)
            self.assertEqual(updates.manifest(packages, "0.2.0", self.public, "Release notes", manifest), manifest)
            with self.assertRaisesRegex(ValueError, "Both supported"):
                updates.manifest({next(iter(packages)):next(iter(packages.values()))}, "0.2.0", self.public, "")
            for previous in ("0.3.0",):
                with self.assertRaisesRegex(ValueError, "roll back"):
                    updates.manifest(packages, "0.2.0", self.public, "", {"version":previous})
            with self.assertRaisesRegex(ValueError, "prerelease"):
                updates.manifest(packages, "0.2.1-beta.1", self.public, "")

    def test_missing_credentials_and_mismatched_private_key_fail_before_build(self):
        with patch.dict(os.environ, {}, clear=True):
            with self.assertRaisesRegex(ValueError, "PUBLIC_KEY"):
                updates.signing_preflight("0.2.0")
        with patch.dict(os.environ, {"MOBILE_CAPTURE_UPDATER_PUBLIC_KEY":self.public}, clear=True):
            with self.assertRaisesRegex(ValueError, "PRIVATE_KEY"):
                updates.signing_preflight("0.2.0")
        environment = {**self.environment, "TAURI_SIGNING_PRIVATE_KEY_PATH": str(self.other_key)}
        with patch.dict(os.environ, environment):
            with self.assertRaises(subprocess.CalledProcessError):
                updates.signing_preflight("0.2.0")

    def test_manifest_rejects_different_embedded_key_dirty_source_and_mutated_metadata(self):
        for field, value, message in [("updaterPublicKeySHA256", "wrong", "different publisher"), ("sourceDirty", True, "provenance"), ("version", "0.1.0", "provenance")]:
            with self.subTest(field=field), tempfile.TemporaryDirectory() as directory:
                packages = self.packages(Path(directory))
                build_path = next(iter(packages.values())) / "BUILD.json"
                build = json.loads(build_path.read_text())
                build[field] = value
                build_path.write_text(json.dumps(build))
                with self.assertRaisesRegex(ValueError, message):
                    updates.manifest(packages, "0.2.0", self.public, "")
        with tempfile.TemporaryDirectory() as directory:
            packages = self.packages(Path(directory))
            metadata_path = next(iter(packages.values())) / "UPDATER.json"
            metadata = json.loads(metadata_path.read_text())
            metadata["artifact"] = "../../untrusted"
            metadata_path.write_text(json.dumps(metadata))
            with self.assertRaisesRegex(ValueError, "identity"):
                updates.manifest(packages, "0.2.0", self.public, "")

    def test_publisher_recovers_every_interrupted_mutation_and_completed_reruns(self):
        for initial in (None, {"version":"0.1.0", "notes":"Older", "pub_date":"2026-10-01T00:00:00Z", "platforms":{}}):
            with tempfile.TemporaryDirectory() as directory:
                directory = Path(directory)
                packages = self.packages(directory)
                baseline = FakeGitHub(initial)
                with patch.object(updates, "run", side_effect=baseline.run), patch.object(updates, "verify"), patch.dict(os.environ, {"GITHUB_SHA":"fixture-commit"}):
                    updates.publish(packages, "0.2.0", self.public, "Notes", directory / "android-updater.json")
                checkpoints = baseline.mutations
                for at in range(1, checkpoints + 1):
                    with self.subTest(initial=bool(initial), interruption=at):
                        github = FakeGitHub(initial, crash_at=at)
                        with patch.object(updates, "run", side_effect=github.run), patch.object(updates, "verify"), patch.dict(os.environ, {"GITHUB_SHA":"fixture-commit"}):
                            with self.assertRaises(SimulatedCrash):
                                updates.publish(packages, "0.2.0", self.public, "Notes", directory / "android-updater.json")
                            updates.publish(packages, "0.2.0", self.public, "Notes", directory / "android-updater.json")
                            self.assertFalse(github.releases[updates.FEED_TAG]["isDraft"])
                            current = json.loads(github.releases[updates.FEED_TAG]["assets"]["android-updater.json"])
                            self.assertEqual(current["version"], "0.2.0")
                            mutations = github.mutations
                            updates.publish(packages, "0.2.0", self.public, "Notes", directory / "android-updater.json")
                            self.assertEqual(github.mutations, mutations, "A completed identical publication must be a no-op")

    def test_feed_upload_failure_restores_previous_feed_and_retry_converges(self):
        previous = {"version":"0.1.0", "notes":"Older", "pub_date":"2026-10-01T00:00:00Z", "platforms":{}}
        with tempfile.TemporaryDirectory() as directory:
            directory = Path(directory)
            packages = self.packages(directory)
            github = FakeGitHub(previous, fail_promotion=True)
            with patch.object(updates, "run", side_effect=github.run), patch.object(updates, "verify"), patch.dict(os.environ, {"GITHUB_SHA":"fixture-commit"}):
                with self.assertRaises(subprocess.CalledProcessError):
                    updates.publish(packages, "0.2.0", self.public, "Notes", directory / "android-updater.json")
                self.assertEqual(json.loads(github.releases[updates.FEED_TAG]["assets"]["android-updater.json"]), previous)
                self.assertIn("android-updater-0.1.0.json", github.releases[updates.FEED_TAG]["assets"])
                updates.publish(packages, "0.2.0", self.public, "Notes", directory / "android-updater.json")
                self.assertEqual(json.loads(github.releases[updates.FEED_TAG]["assets"]["android-updater.json"])["version"], "0.2.0")

    def test_api_outage_keeps_durable_snapshot_and_missing_feed_reconciles_on_retry(self):
        previous = {"version":"0.1.0", "notes":"Older", "pub_date":"2026-10-01T00:00:00Z", "platforms":{}}
        with tempfile.TemporaryDirectory() as directory:
            directory = Path(directory)
            packages = self.packages(directory)
            github = FakeGitHub(previous, fail_promotion=2)
            with patch.object(updates, "run", side_effect=github.run), patch.object(updates, "verify"), patch.dict(os.environ, {"GITHUB_SHA":"fixture-commit"}):
                with self.assertRaisesRegex(RuntimeError, "Immutable feed snapshots remain"):
                    updates.publish(packages, "0.2.0", self.public, "Notes", directory / "android-updater.json")
                self.assertNotIn("android-updater.json", github.releases[updates.FEED_TAG]["assets"])
                self.assertIn("android-updater-0.1.0.json", github.releases[updates.FEED_TAG]["assets"])
                self.assertIn("android-updater-0.2.0.json", github.releases[updates.FEED_TAG]["assets"])
                updates.publish(packages, "0.2.0", self.public, "Notes", directory / "android-updater.json")
                self.assertEqual(json.loads(github.releases[updates.FEED_TAG]["assets"]["android-updater.json"])["version"], "0.2.0")

    def test_public_key_normalization_matches_embedded_runtime_key(self):
        with patch.dict(os.environ, {"MOBILE_CAPTURE_UPDATER_PUBLIC_KEY":"  " + self.public + "\n"}):
            self.assertEqual(updates.public_key(), self.public)

    def test_existing_feed_without_initial_asset_and_corrupt_draft_assets_recover(self):
        with tempfile.TemporaryDirectory() as directory:
            directory = Path(directory)
            packages = self.packages(directory)
            github = FakeGitHub()
            github.releases[updates.FEED_TAG] = {"isDraft":True,"assets":{}}
            github.corrupt_download = True
            with patch.object(updates, "run", side_effect=github.run), patch.object(updates, "verify"), patch.dict(os.environ, {"GITHUB_SHA":"fixture-commit"}):
                with self.assertRaisesRegex(ValueError, "Published artifact differs"):
                    updates.publish(packages, "0.2.0", self.public, "Notes", directory / "android-updater.json")
                self.assertTrue(github.releases["android-v0.2.0"]["isDraft"])
                self.assertTrue(github.releases[updates.FEED_TAG]["isDraft"])
                github.corrupt_download = False
                updates.publish(packages, "0.2.0", self.public, "Notes", directory / "android-updater.json")
                self.assertFalse(github.releases[updates.FEED_TAG]["isDraft"])

    def test_distribution_checksum_refuses_mutation_after_packaging(self):
        with tempfile.TemporaryDirectory() as directory:
            packages = self.packages(Path(directory))
            package = next(iter(packages.values()))
            distribution = package / "manual-install.zip"
            distribution.write_bytes(b"unsigned extra distribution")
            with self.assertRaisesRegex(ValueError, "checksums"):
                updates.manifest(packages, "0.2.0", self.public, "")

    def test_macos_archive_contains_final_payload_and_matching_native_version(self):
        updates.verify_macos_archive(self.artifact, "0.2.0")
        with self.assertRaisesRegex(ValueError, "different native version"):
            updates.verify_macos_archive(self.artifact, "0.3.0")
        with tempfile.TemporaryDirectory() as directory:
            archive = Path(directory) / "incomplete.app.tar.gz"
            with tarfile.open(archive, "w:gz"):
                pass
            with self.assertRaisesRegex(ValueError, "missing the final bundled payload"):
                updates.verify_macos_archive(archive, "0.2.0")

    def test_version_and_tag_mismatch_refuse_release(self):
        with patch.dict(os.environ, {"GITHUB_REF":"refs/tags/android-v999.0.0"}):
            with self.assertRaisesRegex(ValueError, "tag"):
                updates.version()
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            for name in ("package.json", "package-lock.json", "src-tauri/tauri.conf.json", "src-tauri/Cargo.toml", "capture-core/Cargo.toml", "Cargo.lock"):
                destination = root / name
                destination.parent.mkdir(parents=True, exist_ok=True)
                destination.write_bytes((ROOT / name).read_bytes())
            path = root / "src-tauri/tauri.conf.json"
            data = json.loads(path.read_text())
            data["version"] = "999.0.0"
            path.write_text(json.dumps(data))
            with self.assertRaisesRegex(ValueError, "versions disagree"):
                updates.version(root)


if __name__ == "__main__":
    unittest.main()
