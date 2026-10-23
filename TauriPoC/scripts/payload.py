#!/usr/bin/env python3
"""Prepare a complete pinned Android tool payload before Tauri bundling."""
import argparse
import hashlib
import json
import os
from pathlib import Path, PurePosixPath
import shutil
import stat
import struct
import subprocess
import tarfile
import tempfile
from urllib.parse import urlparse
from urllib.request import urlopen
import zipfile
import zlib

ROOT = Path(__file__).resolve().parents[1]
TARGETS = {"aarch64-apple-darwin", "x86_64-pc-windows-msvc"}
MAX_ARCHIVE = 512 * 1024 * 1024
MAX_EXPANDED = 2 * 1024 * 1024 * 1024


def digest(path):
    value = hashlib.sha256()
    with path.open("rb") as handle:
        for block in iter(lambda: handle.read(1024 * 1024), b""):
            value.update(block)
    return value.hexdigest()


def safe_name(value):
    path = PurePosixPath(value)
    if not value or "\\" in value or ":" in value or path.is_absolute() or any(part in ("", ".", "..") for part in value.rstrip("/").split("/")):
        raise ValueError("Unsafe archive path: " + value)
    reserved = {"CON", "PRN", "AUX", "NUL"} | {name + str(number) for name in ("COM", "LPT") for number in range(1, 10)}
    for part in path.parts:
        if part.endswith((".", " ")) or part.upper().split(".")[0] in reserved or any(ord(character) < 32 for character in part):
            raise ValueError("Unsafe portable archive name: " + value)
    return path


def link_target(name, target):
    if not target or "\\" in target or ":" in target or PurePosixPath(target).is_absolute():
        raise ValueError("Unsafe archive link target")
    parts = list(name.parent.parts)
    for part in PurePosixPath(target).parts:
        if part == "..":
            if not parts:
                raise ValueError("Archive link escapes extraction root")
            parts.pop()
        elif part != ".":
            parts.append(part)
    if not parts:
        raise ValueError("Archive link resolves to extraction root")


def extract(archive, destination):
    destination.mkdir(parents=True, exist_ok=False)
    records = []
    if zipfile.is_zipfile(archive):
        with zipfile.ZipFile(archive) as source:
            for entry in source.infolist():
                mode = entry.external_attr >> 16
                if stat.S_ISLNK(mode):
                    raise ValueError("ZIP symlinks are refused")
                records.append((entry.filename, entry.file_size, "directory" if entry.is_dir() else "file", mode, entry))
            _extract_records(records, destination, lambda entry: source.open(entry))
    else:
        with tarfile.open(archive, "r:gz") as source:
            for entry in source.getmembers():
                kind = "directory" if entry.isdir() else "file" if entry.isfile() else "symlink" if entry.issym() else "unsafe"
                records.append((entry.name, entry.size, kind, entry.mode, entry))
            _extract_records(records, destination, lambda entry: source.extractfile(entry))


def _extract_records(records, destination, open_entry):
    total = 0
    names = set()
    for name, size, kind, _, entry in records:
        path = safe_name(name)
        key = str(path).casefold()
        if key in names:
            raise ValueError("Duplicate archive path")
        names.add(key)
        if kind == "unsafe":
            raise ValueError("Archive special files and hard links are refused")
        if kind == "symlink":
            link_target(path, entry.linkname)
        total += size
        if total > MAX_EXPANDED or len(names) > 100000:
            raise ValueError("Archive exceeds extraction bounds")
    links = []
    for name, size, kind, mode, entry in records:
        path = destination.joinpath(*safe_name(name).parts)
        for parent in path.parents:
            if parent == destination:
                break
            if parent.is_symlink():
                raise ValueError("Archive path traverses symlink")
        if kind == "directory":
            path.mkdir(parents=True, exist_ok=True)
        elif kind == "symlink":
            links.append((path, entry.linkname))
        else:
            path.parent.mkdir(parents=True, exist_ok=True)
            with open_entry(entry) as source, path.open("xb") as output:
                copied = 0
                for block in iter(lambda: source.read(1024 * 1024), b""):
                    copied += len(block)
                    if copied > size:
                        raise ValueError("Archive entry exceeds declared size")
                    output.write(block)
                if copied != size:
                    raise ValueError("Incomplete archive entry")
            path.chmod(0o755 if mode & 0o111 else 0o644)
    for path, target in links:
        path.parent.mkdir(parents=True, exist_ok=True)
        path.symlink_to(target)
    root = destination.resolve()
    for path, _ in links:
        if not path.resolve(strict=True).is_relative_to(root):
            raise ValueError("Archive link escapes extraction root")


def download(record, cache):
    cache.mkdir(parents=True, exist_ok=True)
    path = cache / record["filename"]
    if path.is_symlink():
        raise ValueError("Archive cache symlink refused")
    if not path.exists():
        if urlparse(record["url"]).scheme != "https":
            raise ValueError("Dependency download requires HTTPS")
        with tempfile.NamedTemporaryFile(dir=cache, delete=False) as output:
            temporary = Path(output.name)
            try:
                with urlopen(record["url"], timeout=60) as response:
                    if urlparse(response.url).scheme != "https":
                        raise ValueError("Dependency redirect downgraded HTTPS")
                    size = 0
                    for block in iter(lambda: response.read(1024 * 1024), b""):
                        size += len(block)
                        if size > MAX_ARCHIVE:
                            raise ValueError("Dependency archive exceeds bound")
                        output.write(block)
                output.close()
                if digest(temporary) != record["sha256"]:
                    raise ValueError("Dependency archive integrity failed: " + record["filename"])
                temporary.replace(path)
            finally:
                temporary.unlink(missing_ok=True)
    if path.stat().st_size > MAX_ARCHIVE or digest(path) != record["sha256"]:
        raise ValueError("Dependency archive integrity failed: " + record["filename"])
    return path


def embedded_notices(executable, destination):
    binary = executable.read_bytes()
    index = binary.rfind(b"MEI\014\013\012\013\016")
    if index < 0:
        raise ValueError("Standalone Windows runtime archive is missing")
    _, size, offset, length, python_version, _ = struct.unpack("!8sIIII64s", binary[index:index + 88])
    if python_version != 314:
        raise ValueError("Standalone Windows Python runtime changed")
    start = index + 88 - size
    toc = binary[start + offset:start + offset + length]
    count = 0
    while toc:
        length, offset, compressed_size, raw_size, compressed, _ = struct.unpack("!IIIIBc", toc[:18])
        if length < 18 or length > len(toc):
            raise ValueError("Invalid embedded runtime table")
        name = toc[18:length].rstrip(b"\0").decode("utf-8").replace("\\", "/")
        toc = toc[length:]
        if any(word in name.lower() for word in ("license", "copying", "copyright")):
            path = destination.joinpath(*safe_name(name).parts)
            data = binary[start + offset:start + offset + compressed_size]
            if raw_size > 4 * 1024 * 1024:
                raise ValueError("Embedded notice exceeds bound")
            data = zlib.decompress(data) if compressed else data
            if len(data) != raw_size:
                raise ValueError("Incomplete embedded notice")
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(data)
            count += 1
    if not count:
        raise ValueError("Standalone Windows runtime notices are missing")


def sign_macos(root, identity):
    files = []
    for path in root.rglob("*"):
        if path.is_file() and not path.is_symlink():
            with path.open("rb") as handle:
                magic = handle.read(4)
            if magic in (b"\xcf\xfa\xed\xfe", b"\xca\xfe\xba\xbe"):
                files.append(path)
    for path in sorted(files, key=lambda item: len(item.parts), reverse=True):
        command = ["/usr/bin/codesign", "--force", "--sign", identity, "--preserve-metadata=entitlements"]
        if identity != "-":
            command += ["--options", "runtime", "--timestamp"]
        subprocess.run(command + [str(path)], check=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    bundles = [path for path in root.rglob("*") if path.is_dir() and path.suffix in (".framework", ".app")]
    for path in sorted(bundles, key=lambda item: len(item.parts), reverse=True):
        command = ["/usr/bin/codesign", "--force", "--sign", identity, "--preserve-metadata=entitlements"]
        if identity != "-":
            command += ["--options", "runtime", "--timestamp"]
        subprocess.run(command + [str(path)], check=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE)


def inventory(root):
    files = []
    for path in sorted(root.rglob("*")):
        if path.is_symlink():
            target = os.readlink(path)
            files.append({"path": path.relative_to(root).as_posix(), "kind": "symlink", "target": target, "bytes": len(target.encode()), "sha256": hashlib.sha256(target.encode()).hexdigest()})
        elif path.is_file():
            files.append({"path": path.relative_to(root).as_posix(), "kind": "file", "bytes": path.stat().st_size, "sha256": digest(path)})
    return files


def prepare(target, cache, destination, binary_dir, identity=None):
    if target not in TARGETS:
        raise ValueError("Supported targets are macOS ARM64 and Windows x64")
    lock = json.loads((ROOT / "dependencies/downloads.lock.json").read_text())
    if destination.exists() or destination.is_symlink():
        raise ValueError("Payload destination must not exist")
    suffix = ".exe" if target.startswith("x86_64-pc-windows") else ""
    destination.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(dir=destination.parent) as temporary:
        stage = Path(temporary) / "android-tools"
        stage.mkdir()
        for record in lock["targets"][target]:
            archive = download(record, cache)
            extracted = Path(temporary) / record["name"]
            extract(archive, extracted)
            if record["name"] == "platform-tools":
                shutil.move(str(extracted / "platform-tools"), stage / "platform-tools")
            elif suffix:
                shutil.move(str(extracted), stage / "mitmproxy")
            else:
                shutil.move(str(extracted / "mitmproxy.app"), stage / "mitmproxy.app")
        runtime = stage / "runtime"
        runtime.mkdir()
        for name in ("collector-worker", "capture-guardian"):
            source = binary_dir / (name + suffix)
            if not source.is_file() or source.is_symlink():
                raise ValueError("Missing compiled runtime binary: " + str(source))
            shutil.copy2(source, runtime / (name + suffix))
        if suffix and os.environ.get("WINDOWS_CERTIFICATE_THUMBPRINT"):
            for executable in runtime.glob("*.exe"):
                subprocess.run(["signtool", "sign", "/sha1", os.environ["WINDOWS_CERTIFICATE_THUMBPRINT"], "/fd", "SHA256", "/tr", "http://timestamp.digicert.com", "/td", "SHA256", str(executable)], check=True)
        shutil.copy2(ROOT / "resources/network_addon.py", runtime / "network_addon.py")
        notices = stage / "notices"
        notices.mkdir()
        for source in (ROOT.parent / "Dependencies/Capture/licenses").glob("*.txt"):
            shutil.copy2(source, notices / source.name)
        if suffix:
            (notices / "OpenSSL-3.5.5-LICENSE.txt").unlink()
            shutil.copy2(ROOT / "dependencies/OpenSSL-3.0.19-LICENSE.txt", notices)
            embedded_notices(stage / "mitmproxy/mitmdump.exe", notices / "embedded-windows-runtime")
        (notices / "UPSTREAM.txt").write_text("Android SDK Platform-Tools 37.0.1. See platform-tools/NOTICE.txt and source.properties.\nmitmproxy 12.2.3 standalone runtime, Python 3.14.4, and bundled third-party libraries.\nWindows OpenSSL 3.0.19. macOS OpenSSL 3.5.5. Preserve all upstream embedded licenses.\nSource and distribution URLs are recorded in downloads.lock.json.\n")
        shutil.copy2(ROOT / "dependencies/downloads.lock.json", notices)
        if not suffix:
            if identity is None:
                raise ValueError("macOS payload requires an explicit signing identity, '-' for development")
            sign_macos(stage, identity)
        manifest = {"schemaVersion": 1, "target": target, "adbVersion": "37.0.1", "mitmproxyVersion": "12.2.3", "tools": {"adb": "platform-tools/adb" + suffix, "mitmdump": "mitmproxy/mitmdump.exe" if suffix else "mitmproxy.app/Contents/MacOS/mitmdump", "worker": "runtime/collector-worker" + suffix, "guardian": "runtime/capture-guardian" + suffix, "addon": "runtime/network_addon.py"}, "files": inventory(stage)}
        (stage / "payload.json").write_text(json.dumps(manifest, indent=2, sort_keys=True) + "\n")
        stage.rename(destination)
    return digest(destination / "payload.json")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--target", required=True, choices=sorted(TARGETS))
    parser.add_argument("--cache", required=True, type=Path)
    parser.add_argument("--destination", required=True, type=Path)
    parser.add_argument("--binary-dir", required=True, type=Path)
    parser.add_argument("--signing-identity")
    args = parser.parse_args()
    print(prepare(args.target, args.cache.resolve(), args.destination.absolute(), args.binary_dir.resolve(), args.signing_identity))

if __name__ == "__main__":
    main()
