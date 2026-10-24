#!/usr/bin/env python3
"""Read-only schema-1 verifier for full network ZIPs. Never prints payloads."""
from __future__ import annotations

import argparse
import base64
import hashlib
import json
import os
from pathlib import Path
import re
import stat
import subprocess
import sys
import tempfile
import zipfile

RAW = {"logs/device.log": "logs", "network/events.jsonl": "network",
       "timeline/markers.jsonl": "marker"}
OPTIONAL = set(RAW) | {"network/capture.har", "network/capture.mitm", "network/incomplete.json"}
REQUIRED = {"manifest.json", "context.json", "README.txt", "SHA256SUMS", "timeline/events.jsonl",
            "logs/device.log", "timeline/markers.jsonl", "network/events.jsonl",
            "network/capture.har", "network/capture.mitm", "network/incomplete.json"}
ALLOWED = REQUIRED | OPTIONAL
SUM = re.compile(r"([0-9a-f]{64})  ([A-Za-z0-9_./-]+)")
SECRET_KEYS = {"token", "control_token", "auth_token", "access_token", "refresh_token",
               "password", "secret", "client_secret", "api_key", "private_key",
               "authorization", "credentials", "credential", "cookie", "cookies",
               "control", "runtime", "journal", "recovery_journal", "request_path",
               "output_root", "package_path", "directory", "ca_dir", "confdir",
               "public_certificate", "synthetic_check_url"}

class Invalid(ValueError):
    pass

def check(ok, message):
    if not ok:
        raise Invalid(message)
def object_json(data, name):
    try:
        value = json.loads(data)
    except (ValueError, UnicodeError) as exc:
        raise Invalid(f"Invalid JSON: {name}") from exc
    check(isinstance(value, dict), f"Expected JSON object: {name}")
    return value
def check_public_metadata(value):
    if isinstance(value, dict):
        for key, item in value.items():
            lowered = key.lower()
            check(lowered not in SECRET_KEYS and not lowered.endswith(
                ("_token", "_password", "_secret", "_private_key")),
                "Private metadata key exported")
            check_public_metadata(item)
    elif isinstance(value, list):
        for item in value:
            check_public_metadata(item)
def json_lines(data, name):
    rows = []
    for number, raw in enumerate(data.splitlines(), 1):
        if raw.strip():
            rows.append((number, object_json(raw, f"{name}#L{number}"), raw))
    return rows

def raw_lines(data):
    return ((number, raw) for number, raw in enumerate(data.splitlines(), 1) if raw.strip())

def body(value, label):
    check(isinstance(value, dict), f"Missing body object: {label}")
    check(value.get("preview") is not True and "preview_bytes" not in value,
          f"Snapshot preview exported as body evidence: {label}")
    if value.get("unavailable") is True or value.get("_bodyUnavailable") is True:
        check(value.get("text") is None and value.get("bytes") is None and
              value.get("encoding") is None, f"Unavailable body falsely carries bytes: {label}")
        return None
    encoding = value.get("encoding", value.get("_encoding"))
    check(encoding in ("base64", "utf-8"), f"Unsupported body encoding: {label}")
    check(isinstance(value.get("text"), str), f"Missing body text: {label}")
    try:
        data = (base64.b64decode(value["text"], validate=True) if encoding == "base64"
                else value["text"].encode("utf-8"))
    except (ValueError, UnicodeError) as exc:
        raise Invalid(f"Invalid body encoding: {label}") from exc
    check(type(value.get("bytes")) is int and value["bytes"] == len(data),
          f"Body byte count mismatch: {label}")
    check(value.get("byte_basis", "raw_content") == "raw_content", f"Body byte basis mismatch: {label}")
    return data

def har_body(message, field, label):
    check(isinstance(message, dict), f"Missing HAR message: {label}")
    content = message.get(field)
    if message.get("_bodyUnavailable") is True:
        check(message.get("bodySize") == -1, f"Unavailable HAR bodySize mismatch: {label}")
        if field == "content":
            check(isinstance(content, dict) and content.get("size") == -1,
                  f"Unavailable HAR content size mismatch: {label}")
        return None
    if field == "postData" and content is None:
        check(message.get("bodySize") == 0, f"Empty HAR request size mismatch: {label}")
        return b""
    check(isinstance(content, dict), f"Missing HAR body: {label}")
    encoding = content.get("encoding", content.get("_encoding"))
    check(encoding in ("base64", "utf-8") and isinstance(content.get("text"), str),
          f"Invalid HAR body encoding: {label}")
    try:
        data = (base64.b64decode(content["text"], validate=True) if encoding == "base64"
                else content["text"].encode("utf-8"))
    except (ValueError, UnicodeError) as exc:
        raise Invalid(f"Invalid HAR body: {label}") from exc
    check(type(message.get("bodySize")) is int and message["bodySize"] == len(data),
          f"HAR bodySize mismatch: {label}")
    if field == "content":
        check(content.get("size") == len(data), f"HAR content size mismatch: {label}")
    check(message.get("_byteBasis", "raw_content") == "raw_content", f"HAR byte basis mismatch: {label}")
    return data

def native_replay(mitmdump, native):
    addon = '''import hashlib, json, os
from pathlib import Path
flows = {}
def summary(message):
    if message is None: return None
    raw = message.raw_content
    return None if raw is None else [len(raw), hashlib.sha256(raw).hexdigest()]
def request(flow):
    flows[flow.id] = {"request": summary(flow.request), "response": None}
def response(flow):
    flows[flow.id] = {"request": summary(flow.request), "response": summary(flow.response)}
def error(flow):
    flows[flow.id] = {"request": summary(flow.request), "response": summary(flow.response)}
def done():
    Path(os.environ["PROBE_OUT"]).write_text(json.dumps(flows))
'''
    with tempfile.TemporaryDirectory(prefix="tauri-network-probe-") as directory:
        root = Path(directory)
        (root / "capture.mitm").write_bytes(native)
        (root / "proof.py").write_text(addon)
        env = os.environ.copy()
        env["PROBE_OUT"] = str(root / "result.json")
        command = [str(mitmdump), "-n", "-r", str(root / "capture.mitm"), "-s", str(root / "proof.py"),
                   "--set", f"confdir={root / 'confdir'}"]
        result = subprocess.run(command, env=env, cwd=root, capture_output=True, timeout=90)
        check(result.returncode == 0 and (root / "result.json").is_file(),
              f"Native replay failed (mitmdump exit {result.returncode}; output withheld)")
        return object_json((root / "result.json").read_bytes(), "native replay result")

def summary(data):
    return None if data is None else [len(data), hashlib.sha256(data).hexdigest()]

def verify(path, mitmdump=None):
    with zipfile.ZipFile(path) as archive:
        names = archive.namelist()
        check(len(names) == len(set(names)), "Duplicate ZIP entry")
        check(REQUIRED <= set(names), f"Missing required entries: {sorted(REQUIRED - set(names))}")
        check(set(names) <= ALLOWED, f"Unexpected ZIP entries: {sorted(set(names) - ALLOWED)}")
        for info in archive.infolist():
            check(not info.is_dir() and not stat.S_ISLNK(info.external_attr >> 16),
                  "Directory or symlink ZIP entry")
        files = {name: archive.read(name) for name in names}
    sums = files["SHA256SUMS"].decode("ascii").splitlines()
    checked = set()
    for line in sums:
        match = SUM.fullmatch(line)
        check(match is not None, "Malformed SHA256SUMS line")
        digest, name = match.groups()
        check(name in files and name != "SHA256SUMS" and name not in checked, "Invalid checksum path")
        check(hashlib.sha256(files[name]).hexdigest() == digest, f"Checksum mismatch: {name}")
        checked.add(name)
    check(checked == set(files) - {"SHA256SUMS"}, "SHA256SUMS does not cover every file exactly once")
    manifest = object_json(files["manifest.json"], "manifest.json")
    check(manifest.get("format") == "device-capture-evidence" and
          type(manifest.get("schema_version")) is int and manifest["schema_version"] == 1,
          "Unsupported evidence schema")
    context = object_json(files["context.json"], "context.json")
    check(manifest.get("routing_restored") is True, "Routing restoration is not verified")
    check_public_metadata(manifest)
    check_public_metadata(context)
    artifacts = manifest.get("artifacts")
    check(isinstance(artifacts, list), "Missing artifact manifest")
    actual_raw = set(files) & OPTIONAL
    recorded = set()
    for item in artifacts:
        check(isinstance(item, dict) and item.get("path") in actual_raw, "Unexpected artifact manifest path")
        name = item["path"]
        check(name not in recorded and item.get("bytes") == len(files[name]) and
              item.get("sha256") == hashlib.sha256(files[name]).hexdigest(),
              f"Artifact manifest mismatch: {name}")
        recorded.add(name)
    check(recorded == actual_raw, "Artifact manifest does not cover raw files")

    expected = {}
    raw_network = {}
    for path_name, source in RAW.items():
        if path_name not in files:
            continue
        for number, raw in raw_lines(files[path_name]):
            ref = f"{path_name}#L{number}"
            identity = hashlib.sha256(source.encode() + b"\0" + path_name.encode() + b"\0" +
                                      str(number).encode() + b"\0" + raw).hexdigest()[:24]
            expected[ref] = (source, "event-" + identity)
            if source == "network":
                try:
                    raw_network[ref] = object_json(raw, ref)
                except Invalid:
                    raw_network[ref] = None
    timeline = json_lines(files["timeline/events.jsonl"], "timeline/events.jsonl")
    events = {}
    counts = {"logs": 0, "network": 0, "markers": 0}
    for _, event, _ in timeline:
        ref = event.get("raw_ref")
        check(ref in expected and ref not in events, "Timeline has missing or duplicate raw reference")
        source, identity = expected[ref]
        check(event.get("source") == source and event.get("id") == identity,
              f"Noncanonical timeline source or ID: {ref}")
        check(isinstance(event.get("detail"), dict), f"Missing timeline detail: {ref}")
        events[ref] = event
        counts["markers" if source == "marker" else source] += 1
    check(set(events) == set(expected), "Timeline omitted raw evidence")
    stated = manifest.get("counts", {})
    check(all(stated.get(key) == value for key, value in counts.items()),
          "Manifest event counts mismatch")
    for ref, raw in raw_network.items():
        if raw is None:
            check(events[ref].get("kind") == "parse_error" and
                  isinstance(events[ref]["detail"].get("parse_error"), str),
                  f"Malformed network source was not marked parse_error: {ref}")
    parse_errors = [ref for ref, raw in raw_network.items() if raw is None]
    check(not parse_errors or manifest.get("package_state") == "partial",
          "Malformed network source claimed complete")
    network = [event for event in events.values() if event["source"] == "network"]
    decoded = sum(event.get("kind") in ("request", "response") for event in network)
    net_status = manifest.get("completeness", {}).get("sources", {}).get("network", {})
    check(net_status.get("requested") is True and net_status.get("events") == counts["network"] and
          net_status.get("decoded_http_events") == decoded and
          net_status.get("observed") is (decoded > 0), "Network coverage claim mismatch")
    check(manifest.get("sources", {}).get("network") != "observed" or decoded > 0,
          "TLS-only evidence claimed as observed HTTP")
    diagnostics = manifest.get("network_diagnostics", {})
    check("decoded_http_events" not in diagnostics or diagnostics["decoded_http_events"] == decoded,
          "Network diagnostic count mismatch")

    incomplete = object_json(files["network/incomplete.json"], "network/incomplete.json")
    check(isinstance(incomplete.get("flow_ids"), list), "Missing pending flow list")
    check(incomplete.get("count", len(incomplete["flow_ids"])) == len(incomplete["flow_ids"]),
          "Pending flow count mismatch")
    pending = set(incomplete["flow_ids"])
    check(all(isinstance(item, str) for item in pending), "Invalid pending flow ID")
    manifest_pending = manifest.get("completeness", {}).get("pending_flow_ids", [])
    check(pending <= set(manifest_pending), "Manifest omitted pending flows")
    har = object_json(files["network/capture.har"], "network/capture.har")
    entries = har.get("log", {}).get("entries")
    check(isinstance(entries, list), "Missing HAR entries")
    flows = {}
    for entry in entries:
        flow = entry.get("_flow_id") if isinstance(entry, dict) else None
        check(isinstance(flow, str) and flow and flow not in flows, "Missing or duplicate HAR flow ID")
        request = har_body(entry.get("request"), "postData", "HAR request")
        response_obj = entry.get("response", {})
        if response_obj.get("status") == 0:
            check(entry.get("_incomplete") is True and response_obj.get("bodySize") == -1,
                  "Unfinished HAR flow mismatch")
            response = None
        else:
            response = har_body(response_obj, "content", "HAR response")
        flows[flow] = {"request": request, "response": response,
                       "has_response": response_obj.get("status") != 0}
    by_flow = {}
    for ref, raw in raw_network.items():
        if raw is None:
            continue
        kind = raw.get("kind")
        if kind not in ("request", "response", "error"):
            continue
        detail = raw.get("detail", {})
        flow = detail.get("flow_id")
        check(flow in flows, f"Raw network event has no HAR flow: {ref}")
        key = (flow, kind)
        check(key not in by_flow, f"Duplicate raw network flow event: {ref}")
        by_flow[key] = ref
        for field, expected_body in (("request_body", flows[flow]["request"]),
                                     ("response_body", flows[flow]["response"])):
            if field == "response_body" and kind != "response":
                continue
            actual = body(detail.get(field), f"raw {field}")
            check(actual == expected_body, f"Raw {field} differs from HAR: {ref}")
            normalized = events[ref]["detail"].get(field)
            check(normalized == detail[field], f"Timeline replaced raw {field}: {ref}")
    for flow, value in flows.items():
        check((flow, "request") in by_flow, "HAR flow lacks raw request event")
        check(not value["has_response"] or (flow, "response") in by_flow,
              "HAR response lacks raw response event")
    missing = incomplete.get("missing_bodies", [])
    check(isinstance(missing, list), "Invalid missing body list")
    unavailable = sum(value[side] is None for value in flows.values() for side in
                      ("request", "response") if side == "request" or value["has_response"])
    check(unavailable <= len(missing), "Unavailable body missing from gap report")
    service = manifest.get("network_completion", manifest.get("network_service"))
    service = service if isinstance(service, dict) else {}
    check("routing_restored" not in service or service["routing_restored"] is True,
          "Network completion disagrees with routing restoration")
    source_statuses = list(manifest.get("sources", {}).values()) + [
        item.get("runtime_status") for item in manifest.get("completeness", {}).get("sources", {}).values()
        if isinstance(item, dict)]
    gaps = bool(pending or missing or incomplete.get("omitted_flows") or incomplete.get("errors") or
                manifest.get("completeness", {}).get("missing_artifacts") or
                manifest.get("completeness", {}).get("partial") or
                manifest.get("phase") in ("failed", "partial", "preparing", "recording", "stopping") or
                any(value in ("failed", "interrupted", "unavailable", "tls_failed") for value in source_statuses) or
                any(service.get(key) for key in ("forced", "interrupted", "partial")))
    check(not gaps or manifest.get("package_state") == "partial", "Known gap claimed complete")
    check(manifest.get("package_state") in ("complete", "partial"), "Invalid package state")
    if mitmdump:
        native = native_replay(mitmdump, files["network/capture.mitm"])
        check(set(native) == set(flows), "Native flow IDs differ from HAR")
        for flow, values in flows.items():
            check(native[flow].get("request") == summary(values["request"]), "Native request bytes differ")
            check(native[flow].get("response") == summary(values["response"]), "Native response bytes differ")
    return {"archive_sha256": hashlib.sha256(path.read_bytes()).hexdigest(),
            "files": len(files), "events": counts, "decoded_http_events": decoded,
            "har_flows": len(flows), "unavailable_bodies": unavailable,
            "pending_flows": len(pending), "package_state": manifest["package_state"],
            "native_replay": bool(mitmdump)}

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("archive", type=Path, help="capture-package.zip")
    parser.add_argument("--mitmdump", type=Path, help="replay native flows offline with mitmdump")
    args = parser.parse_args()
    try:
        print(json.dumps(verify(args.archive, args.mitmdump), sort_keys=True))
    except (Invalid, OSError, UnicodeError, TypeError, KeyError, AttributeError, zipfile.BadZipFile, subprocess.TimeoutExpired) as exc:
        print(f"ARCHIVE INVALID: {exc}", file=sys.stderr)
        raise SystemExit(1)

if __name__ == "__main__":
    main()
