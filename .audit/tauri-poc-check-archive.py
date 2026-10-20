#!/usr/bin/env python3

import argparse
import hashlib
import json
from pathlib import Path
import sys
import tempfile
import zipfile

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "CaptureRuntime"))
import evidence


def require(condition, message):
    if not condition:
        raise ValueError(message)


def check_archive(archive_path, session):
    required = {"manifest.json", "context.json", "README.txt", "SHA256SUMS",
                "logs/device.log", "timeline/events.jsonl"}
    allowed = required | {"timeline/markers.jsonl", "ENGINEER-SUMMARY.txt"}
    with zipfile.ZipFile(archive_path) as archive:
        names = archive.namelist()
        require(len(names) == len(set(names)), "duplicate archive path")
        require(required <= set(names) <= allowed, "unexpected or missing archive path")
        files = {name: archive.read(name) for name in names}
    checksums = {}
    for line in files["SHA256SUMS"].decode().splitlines():
        digest, name = line.split("  ", 1)
        require(name not in checksums, "duplicate checksum path")
        checksums[name] = digest
    require(set(checksums) == set(files) - {"SHA256SUMS"}, "checksum coverage mismatch")
    for name, digest in checksums.items():
        require(hashlib.sha256(files[name]).hexdigest() == digest, "checksum mismatch: " + name)
    for name in ("logs/device.log", "timeline/markers.jsonl"):
        source = session / name
        require(source.exists() == (name in files), "raw file presence mismatch: " + name)
        if name in files:
            require(source.read_bytes() == files[name], "raw bytes mismatch: " + name)
    manifest = json.loads(files["manifest.json"])
    require(manifest.get("format") == "device-capture-evidence", "wrong evidence format")
    require(manifest.get("schema_version") == 1, "wrong evidence schema")
    require(manifest.get("capture_network") is False, "network selection mismatch")
    require(manifest.get("sources", {}).get("network") == "not_selected", "network source mismatch")
    events = [json.loads(line) for line in files["timeline/events.jsonl"].splitlines() if line.strip()]
    raw = {name: data for name, data in files.items() if name in evidence.RAW_FILES}
    canonical = evidence._read_events(session, manifest, raw)
    fields = ("id", "source", "kind", "raw_ref", "summary", "original_timestamp")
    observed = [{key: row.get(key) for key in fields} for row in events]
    expected = [{key: row.get(key) for key in fields} for row in canonical]
    require(observed == expected, "canonical event mismatch")
    counts = {"logs": sum(row["source"] == "logs" for row in events),
              "network": 0, "markers": sum(row["source"] == "marker" for row in events)}
    require(all(manifest.get("counts", {}).get(key) == count for key, count in counts.items()),
            "manifest count mismatch")
    return {"archive": str(archive_path), "verified": True, "counts": counts,
            "package_state": manifest.get("package_state"), "files": sorted(files)}


def write_control(path, files):
    with zipfile.ZipFile(path, "w") as archive:
        for name, data in files.items():
            archive.writestr(name, data)


def refresh_checksums(files):
    files["SHA256SUMS"] = "".join(
        hashlib.sha256(data).hexdigest() + "  " + name + "\n"
        for name, data in sorted(files.items()) if name != "SHA256SUMS"
    ).encode()


def self_test():
    with tempfile.TemporaryDirectory(prefix="tauri-archive-check-") as temporary:
        session = Path(temporary)
        (session / "logs").mkdir()
        (session / "timeline").mkdir()
        (session / "logs/device.log").write_bytes(b"1792022401.000  123  456 I Proof: recorded\n")
        (session / "timeline/markers.jsonl").write_text(json.dumps({
            "timestamp": "2026-10-15T00:00:02.000000Z", "text": "First\nSecond"
        }) + "\n")
        manifest = {"id": "archive-check", "platform": "android", "device_id": "fixture",
                    "device_name": "Fixture", "started_at": "2026-10-15T00:00:00Z",
                    "phase": "complete", "capture_logs": True, "capture_network": False,
                    "sources": {"logs": "observed", "network": "not_selected"},
                    "routing_restored": True}
        original = evidence.finalize_package(session, manifest)
        check_archive(original, session)
        with zipfile.ZipFile(original) as archive:
            baseline = {name: archive.read(name) for name in archive.namelist()}
        cases = []
        raw = dict(baseline)
        raw["logs/device.log"] += b"changed\n"
        refresh_checksums(raw)
        cases.append((raw, "raw bytes mismatch"))
        identity = dict(baseline)
        rows = [json.loads(line) for line in identity["timeline/events.jsonl"].splitlines()]
        rows[0]["id"] = "event-wrong"
        identity["timeline/events.jsonl"] = b"".join(json.dumps(row).encode() + b"\n" for row in rows)
        refresh_checksums(identity)
        cases.append((identity, "canonical event mismatch"))
        digest = dict(baseline)
        digest["README.txt"] += b"changed"
        cases.append((digest, "checksum mismatch"))
        private = dict(baseline)
        private["runtime/worker.json"] = b"{}"
        cases.append((private, "unexpected or missing archive path"))
        count = dict(baseline)
        metadata = json.loads(count["manifest.json"])
        metadata["counts"]["logs"] += 1
        count["manifest.json"] = json.dumps(metadata).encode()
        refresh_checksums(count)
        cases.append((count, "manifest count mismatch"))
        for index, (files, expected_error) in enumerate(cases):
            control = session / ("control-" + str(index) + ".zip")
            write_control(control, files)
            try:
                check_archive(control, session)
            except ValueError as error:
                require(expected_error in str(error), "negative control failed for an unrelated reason")
            else:
                raise ValueError("negative control was accepted: " + expected_error)
        return {"verified": True, "canonical_package_accepted": True,
                "negative_controls_rejected": len(cases)}


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--self-test", action="store_true")
    parser.add_argument("--session", type=Path)
    parser.add_argument("--archive", type=Path)
    arguments = parser.parse_args()
    if arguments.self_test:
        result = self_test()
    else:
        if arguments.session is None or arguments.archive is None:
            parser.error("--session and --archive are required")
        result = check_archive(arguments.archive, arguments.session)
    print(json.dumps(result, indent=2))


if __name__ == "__main__":
    main()
