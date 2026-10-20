"""Compare the Rust PoC archive against the existing schema-1 normalizer."""
import hashlib
import json
from pathlib import Path
import sys
import zipfile

sys.path.insert(0, str(Path(__file__).resolve().parents[2]))
from CaptureRuntime.evidence import RAW_FILES, _read_events

archive = Path(sys.argv[1])
with zipfile.ZipFile(archive) as package:
    names = set(package.namelist())
    checked = set()
    for line in package.read("SHA256SUMS").decode().splitlines():
        digest, name = line.split("  ")
        assert hashlib.sha256(package.read(name)).hexdigest() == digest, name
        checked.add(name)
    assert checked == names - {"SHA256SUMS"}
    manifest = json.loads(package.read("manifest.json"))
    assert manifest["format"] == "device-capture-evidence"
    assert manifest["schema_version"] == 1
    assert manifest["context"]["fixture"] is True
    assert manifest["context"]["log_scope"]["explicit_consent"] is True
    assert manifest["sources"]["network"] == "not_selected"
    assert manifest["package_state"] == "partial"
    assert not names & {"session.json", "receipt.json", "receipt.pending"}
    rust = [json.loads(line) for line in package.read("timeline/events.jsonl").splitlines()]
    raw = {name: package.read(name) for name in names if name in RAW_FILES}
    python = _read_events(archive.parent, manifest, raw)
    assert len(rust) == len(python)
    for actual, expected in zip(rust, python):
        for field in ("id", "timestamp", "original_timestamp", "received_at", "source", "kind", "summary", "raw_ref", "clock_uncertainty_ms"):
            assert actual[field] == expected[field], (field, actual, expected)
        assert actual["detail"]["timestamp_basis"] == expected["detail"]["timestamp_basis"]
    print(f"Verified schema 1 ZIP, all checksums, and {len(rust)} Rust events against the existing Python normalizer.")
