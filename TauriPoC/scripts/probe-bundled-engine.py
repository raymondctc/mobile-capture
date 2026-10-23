#!/usr/bin/env python3
"""Prove capture with the pinned engine from the complete desktop package."""
import argparse
import json
import os
from pathlib import Path
import re
import subprocess
import sys

from payload import ROOT, digest, inventory, safe_name


def bundled_engine(package, payload):
    payload = payload.resolve(strict=True)
    manifest_path = payload / "payload.json"
    manifest = json.loads(manifest_path.read_text())
    build = json.loads((package / "BUILD.json").read_text())
    lock = json.loads((ROOT / "dependencies/downloads.lock.json").read_text())
    if digest(manifest_path) != build["payloadSHA256"] or manifest_path.read_bytes() != (package / "payload.json").read_bytes():
        raise ValueError("Installed payload differs from the packaged payload")
    if manifest["target"] != build["target"] or manifest["mitmproxyVersion"] != lock["mitmproxyVersion"]:
        raise ValueError("Packaged engine differs from the pinned target or version")
    files = [entry for entry in inventory(payload) if entry["path"] != "payload.json"]
    if files != manifest["files"]:
        raise ValueError("Installed payload inventory differs from the package manifest")
    engine = payload.joinpath(*safe_name(manifest["tools"]["mitmdump"]).parts).resolve(strict=True)
    if not engine.is_relative_to(payload):
        raise ValueError("Packaged engine escapes the payload")
    result = subprocess.run([str(engine), "--version"], check=True, capture_output=True, text=True, timeout=30)
    version = re.search(r"(?m)^Mitmproxy:\s*(\S+)(?:\s+binary)?\s*$", result.stdout)
    if version is None or version[1] != lock["mitmproxyVersion"]:
        raise ValueError("Bundled mitmdump reports an unexpected version: " + result.stdout)
    return engine


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--package", required=True, type=Path)
    parser.add_argument("--payload", required=True, type=Path)
    args = parser.parse_args()
    engine = bundled_engine(args.package, args.payload)
    env = dict(os.environ, CAPTURE_TEST_MITMDUMP=str(engine), CAPTURE_TEST_LONG_POLICY="1")
    subprocess.run([sys.executable, str(ROOT / "scripts/probe-network-engine.py")], cwd=ROOT, env=env, check=True)


if __name__ == "__main__":
    main()
