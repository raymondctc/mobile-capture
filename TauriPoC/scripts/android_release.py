"""Version, final-byte signature and Android-only release feed contract."""
import argparse
import base64
import hashlib
import json
import os
import plistlib
import tarfile
from pathlib import Path
import re
import subprocess
import tempfile
from datetime import datetime, timezone

ROOT = Path(__file__).resolve().parents[1]
REPOSITORY = "raymondctc/mobile-capture"
FEED_TAG = "android-updates-stable"
TARGETS = {"aarch64-apple-darwin": "darwin-aarch64", "x86_64-pc-windows-msvc": "windows-x86_64"}


def stable_version(value):
    if not isinstance(value, str) or not re.fullmatch(r"(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)", value):
        raise ValueError("Android stable releases require an X.Y.Z version without a prerelease or build suffix")
    return tuple(map(int, value.split(".")))


def version(root=ROOT):
    value = json.loads((root / "package.json").read_text())["version"]
    stable_version(value)
    copies = [json.loads((root / "package-lock.json").read_text())["version"], json.loads((root / "package-lock.json").read_text())["packages"][""]["version"], json.loads((root / "src-tauri/tauri.conf.json").read_text())["version"]]
    for manifest in ("src-tauri/Cargo.toml", "capture-core/Cargo.toml"):
        copies.append(re.search(r'^version = "([^"]+)"', (root / manifest).read_text(), re.M)[1])
    lock = (root / "Cargo.lock").read_text()
    for name in ("capture-poc-core", "mobile-capture-tauri-poc"):
        copies.append(re.search(r'name = "' + name + r'"\nversion = "([^"]+)"', lock)[1])
    if any(copy != value for copy in copies):
        raise ValueError("Android package, npm lock, Tauri and Cargo versions disagree")
    tag = os.environ.get("GITHUB_REF", "")
    if tag.startswith("refs/tags/") and tag != "refs/tags/android-v" + value:
        raise ValueError("Android release tag and application version disagree")
    return value


def public_key():
    value = os.environ.get("MOBILE_CAPTURE_UPDATER_PUBLIC_KEY", "").strip()
    if not value:
        raise ValueError("MOBILE_CAPTURE_UPDATER_PUBLIC_KEY is required")
    decoded = base64.b64decode(value, validate=True).decode()
    lines = decoded.splitlines()
    if len(lines) != 2 or not lines[0].startswith("untrusted comment:") or len(base64.b64decode(lines[1], validate=True)) != 42:
        raise ValueError("The updater public key must be the Tauri-generated base64 .pub file")
    return value


def run(command, **kwargs):
    return subprocess.run(command, cwd=ROOT, check=True, **kwargs)


def verify(artifact, signature, key, release_version):
    with tempfile.TemporaryDirectory() as directory:
        key_path = Path(directory) / "publisher.pub"
        key_path.write_text(key)
        run(["cargo", "run", "--quiet", "--locked", "-p", "capture-poc-core", "--bin", "verify-update", "--", str(artifact), str(signature), str(key_path), release_version])


def sign(artifact, release_version):
    if not os.environ.get("TAURI_SIGNING_PRIVATE_KEY") and not os.environ.get("TAURI_SIGNING_PRIVATE_KEY_PATH"):
        raise ValueError("TAURI_SIGNING_PRIVATE_KEY is required for release signing")
    if "TAURI_SIGNING_PRIVATE_KEY_PASSWORD" not in os.environ:
        raise ValueError("TAURI_SIGNING_PRIVATE_KEY_PASSWORD must be set, including for an empty password")
    run(["node", str(ROOT / "node_modules/@tauri-apps/cli/tauri.js"), "signer", "sign", "--app-version", release_version, str(artifact)], stdout=subprocess.DEVNULL)
    signature = Path(str(artifact) + ".sig")
    verify(artifact, signature, public_key(), release_version)
    return signature.read_text().strip()


def signing_preflight(release_version):
    public_key()
    with tempfile.TemporaryDirectory() as directory:
        proof = Path(directory) / "publisher-key-proof"
        proof.write_bytes(b"Mobile Capture Android release signing preflight\n")
        sign(proof, release_version)


def sha256(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def verify_checksums(directory):
    records = {}
    for line in (directory / "SHA256SUMS").read_text().splitlines():
        digest, name = line.split("  ", 1)
        if Path(name).name != name or not re.fullmatch(r"[a-f0-9]{64}", digest) or name in records:
            raise ValueError("Invalid package checksum manifest")
        records[name] = digest
    expected = {path.name for path in directory.iterdir() if path.is_file() and path.name != "SHA256SUMS"}
    if set(records) != expected or any(sha256(directory / name) != digest for name, digest in records.items()):
        raise ValueError("Package checksums differ from final distribution bytes")


def verify_macos_archive(artifact, release_version):
    with tarfile.open(artifact, "r:gz") as archive:
        members = archive.getnames()
        prefix = "Mobile Capture Android.app/Contents/"
        if prefix + "Resources/android-tools/payload.json" not in members:
            raise ValueError("The updater archive is missing the final bundled payload")
        with archive.extractfile(prefix + "Info.plist") as source:
            if plistlib.load(source).get("CFBundleShortVersionString") != release_version:
                raise ValueError("The updater archive contains a different native version")


def manifest(packages, release_version, key, notes, previous=None):
    stable_version(release_version)
    if previous is not None and stable_version(previous["version"]) > stable_version(release_version):
        raise ValueError("Feed promotion cannot roll back the Android stable version")
    if set(packages) != set(TARGETS):
        raise ValueError("Both supported platforms must finish before feed promotion")
    platforms = {}
    commits = set()
    for target, directory in packages.items():
        build = json.loads((directory / "BUILD.json").read_text())
        metadata = json.loads((directory / "UPDATER.json").read_text())
        if build.get("version") != release_version or build.get("target") != target or build.get("channel") != "release" or build.get("sourceDirty") is not False:
            raise ValueError("Package provenance does not match the signed release")
        if build.get("updaterPublicKeySHA256") != hashlib.sha256(key.encode()).hexdigest():
            raise ValueError("The package embedded a different publisher key")
        commits.add(build["commit"])
        expected = f"MobileCaptureAndroid-{release_version}-" + ("Windows-x64.exe" if target.startswith("x86_64") else "macOS-arm64.app.tar.gz")
        if metadata.get("version") != release_version or metadata.get("target") != target or metadata.get("artifact") != expected:
            raise ValueError("Updater artifact identity does not match the release")
        artifact = directory / expected
        signature = Path(str(artifact) + ".sig")
        if sha256(artifact) != metadata["sha256"] or signature.read_text().strip() != metadata["signature"]:
            raise ValueError("Final updater bytes or signature were changed")
        verify_checksums(directory)
        verify(artifact, signature, key, release_version)
        if target == "aarch64-apple-darwin":
            verify_macos_archive(artifact, release_version)
        platforms[TARGETS[target]] = {"url": f"https://github.com/{REPOSITORY}/releases/download/android-v{release_version}/{expected}", "signature": metadata["signature"]}
    if len(commits) != 1:
        raise ValueError("Platform packages were built from different commits")
    result = {"version": release_version, "notes": notes, "pub_date": datetime.now(timezone.utc).isoformat(), "platforms": platforms}
    if previous is not None and previous["version"] == release_version:
        if previous["platforms"] != platforms or previous["notes"] != notes:
            raise ValueError("The same Android version has different release contents")
        return previous
    return result


def github(*arguments, **kwargs):
    return run(["gh", "release", *arguments, "--repo", REPOSITORY], **kwargs)


def release_inventory():
    records = json.loads(github("list", "--limit", "1000", "--json", "tagName,isDraft", capture_output=True, text=True).stdout)
    return {record["tagName"]: record for record in records}


def asset_names(tag):
    info = json.loads(github("view", tag, "--json", "assets", capture_output=True, text=True).stdout)
    return {asset["name"] for asset in info["assets"]}


def download_asset(tag, name, directory):
    directory.mkdir(parents=True, exist_ok=True)
    github("download", tag, "--pattern", name, "--dir", str(directory))
    return directory / name


def snapshot_name(feed):
    stable_version(feed["version"])
    return "android-updater-" + feed["version"] + ".json"


def ensure_snapshot(feed, directory):
    name = snapshot_name(feed)
    local = directory / name
    local.write_text(json.dumps(feed, indent=2) + "\n")
    if name not in asset_names(FEED_TAG):
        github("upload", FEED_TAG, str(local))
    remote = download_asset(FEED_TAG, name, directory / ("verify-" + feed["version"]))
    if json.loads(remote.read_text()) != feed:
        raise ValueError("Immutable feed snapshot differs from the verified release")
    return local


def publish(packages, release_version, key, notes, feed_path):
    with tempfile.TemporaryDirectory() as temporary:
        temporary = Path(temporary)
        inventory = release_inventory()
        feed_release = inventory.get(FEED_TAG)
        canonical = None
        previous = None
        if feed_release:
            names = asset_names(FEED_TAG)
            if "android-updater.json" in names:
                canonical = download_asset(FEED_TAG, "android-updater.json", temporary / "previous")
                previous = json.loads(canonical.read_text())
            snapshots = [name for name in names if re.fullmatch(r"android-updater-[0-9]+\.[0-9]+\.[0-9]+\.json", name)]
            if snapshots:
                name = max(snapshots, key=lambda name: stable_version(name.removeprefix("android-updater-").removesuffix(".json")))
                value = json.loads(download_asset(FEED_TAG, name, temporary / "history").read_text())
                if snapshot_name(value) != name:
                    raise ValueError("Feed snapshot version and name disagree")
                if previous is None or stable_version(value["version"]) > stable_version(previous["version"]):
                    previous = value
        feed = manifest(packages, release_version, key, notes, previous)
        feed_path.write_text(json.dumps(feed, indent=2) + "\n")
        tag = "android-v" + release_version
        version_release = inventory.get(tag)
        if version_release is None:
            github("create", tag, "--verify-tag", "--draft", "--latest=false", "--title", "Mobile Capture Android " + release_version, "--notes", notes)
        assets = []
        for target, directory in packages.items():
            assets.extend(path for path in directory.iterdir() if path.is_file() and path.name.startswith("MobileCaptureAndroid-"))
            provenance = temporary / ("BUILD-" + target + ".json")
            provenance.write_bytes((directory / "BUILD.json").read_bytes())
            assets.append(provenance)
        checksums = temporary / "SHA256SUMS"
        checksums.write_text("".join(sha256(path) + "  " + path.name + "\n" for path in sorted(assets, key=lambda path: path.name)))
        assets.append(checksums)
        if version_release is None or version_release["isDraft"]:
            github("upload", tag, "--clobber", *map(str, assets))
        for asset in assets:
            remote = download_asset(tag, asset.name, temporary / "remote-version")
            if sha256(remote) != sha256(asset):
                raise ValueError("Published artifact differs from verified final bytes")
        if version_release is None or version_release["isDraft"]:
            github("edit", tag, "--draft=false", "--latest=false")
        if feed_release is None:
            github("create", FEED_TAG, "--draft", "--title", "Android stable update feed", "--notes", "Android desktop updater feed. Versioned packages are in android-v releases.", "--latest=false", "--target", os.environ["GITHUB_SHA"])
        if canonical and json.loads(canonical.read_text())["version"] != release_version:
            ensure_snapshot(json.loads(canonical.read_text()), temporary)
        ensure_snapshot(feed, temporary)
        unchanged = canonical is not None and json.loads(canonical.read_text()) == feed
        if not unchanged:
            try:
                github("upload", FEED_TAG, str(feed_path), "--clobber")
                remote = download_asset(FEED_TAG, "android-updater.json", temporary / "verify-canonical")
                if json.loads(remote.read_text()) != feed:
                    raise ValueError("Promoted feed differs from the verified release")
            except (subprocess.CalledProcessError, ValueError) as error:
                if canonical:
                    try:
                        github("upload", FEED_TAG, str(canonical), "--clobber")
                    except subprocess.CalledProcessError:
                        raise RuntimeError("Feed promotion and restoration failed. Immutable feed snapshots remain available. Rerun the same publication after GitHub recovers.") from error
                raise
        if feed_release is None or feed_release["isDraft"]:
            github("edit", FEED_TAG, "--draft=false", "--latest=false")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--packages", type=Path)
    parser.add_argument("--notes", type=Path)
    parser.add_argument("--publish", action="store_true")
    args = parser.parse_args()
    release_version = version()
    if args.packages is None:
        print(release_version)
        return
    key = public_key()
    packages = {}
    for build_file in args.packages.glob("*/BUILD.json"):
        target = json.loads(build_file.read_text())["target"]
        if target in packages:
            raise ValueError("Duplicate platform package")
        packages[target] = build_file.parent
    commit = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip()
    if any(json.loads((directory / "BUILD.json").read_text())["commit"] != commit for directory in packages.values()):
        raise ValueError("Platform packages do not match the publishing checkout")
    notes = args.notes.read_text() if args.notes else "Mobile Capture Android " + release_version
    feed_path = args.packages / "android-updater.json"
    if args.publish:
        if os.environ.get("GITHUB_REF") != "refs/tags/android-v" + release_version:
            raise ValueError("Publication requires the matching Android release tag")
        publish(packages, release_version, key, notes, feed_path)
    else:
        feed_path.write_text(json.dumps(manifest(packages, release_version, key, notes), indent=2) + "\n")
        print(feed_path)


if __name__ == "__main__":
    main()
