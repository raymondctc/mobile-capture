#!/usr/bin/env python3
"""Set the Android desktop version in npm, Tauri and Cargo metadata."""
import argparse
import json
import re
from android_release import ROOT, stable_version, version

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("version")
args = parser.parse_args()
stable_version(args.version)
for name in ("package.json", "package-lock.json", "src-tauri/tauri.conf.json"):
    path = ROOT / name
    data = json.loads(path.read_text())
    data["version"] = args.version
    if name == "package-lock.json":
        data["packages"][""]["version"] = args.version
    path.write_text(json.dumps(data, indent=2) + "\n")
for name in ("src-tauri/Cargo.toml", "capture-core/Cargo.toml"):
    path = ROOT / name
    path.write_text(re.sub(r'^version = "[^"]+"', f'version = "{args.version}"', path.read_text(), count=1, flags=re.M))
path = ROOT / "Cargo.lock"
content = path.read_text()
for package in ("capture-poc-core", "mobile-capture-tauri-poc"):
    content = re.sub(r'(name = "' + package + r'"\nversion = ")[^"]+', lambda match: match[1] + args.version, content)
path.write_text(content)
print(version())
