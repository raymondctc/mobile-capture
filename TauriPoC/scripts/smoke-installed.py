#!/usr/bin/env python3
"""Run the installed console Doctor without developer tools or a phone."""
import argparse
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--app", required=True, type=Path)
    args = parser.parse_args()
    app = args.app.resolve(strict=True)
    macos = sys.platform == "darwin"
    doctor = app / ("Contents/MacOS/android-doctor" if macos else "android-doctor.exe")
    resources = app / "Contents/Resources" if macos else app
    with tempfile.TemporaryDirectory(prefix="android-doctor-installed-") as storage:
        if not macos:
            subprocess.run(["icacls", storage, "/inheritance:r", "/grant:r", os.environ["USERNAME"] + ":(OI)(CI)F"], check=True, capture_output=True)
        env = os.environ.copy()
        env["PATH"] = ""
        env.pop("PYTHONPATH", None)
        env.pop("PYTHONHOME", None)
        command = [str(doctor), "--json", "--resource-dir", str(resources), "--storage-dir", storage]
        result = subprocess.run(command, env=env, capture_output=True, text=True, timeout=100)
        report = json.loads(result.stdout)
        if result.returncode != 0 or not report["hostPrerequisitesReady"]:
            raise RuntimeError(json.dumps(report, indent=2) + "\n" + result.stderr)
        assert report["deviceSetup"]["status"] == "notChecked"
        assert all(check["source"] in ("bundled", "system") for check in report["checks"])
        assert report["schemaVersion"] == 1
        assert not list(Path(storage).iterdir())
        print(json.dumps(report, indent=2))
        invalid = subprocess.run(command + ["--json"], env=env, capture_output=True, text=True, timeout=10)
        assert invalid.returncode == 64
        assert json.loads(invalid.stdout)["error"]["code"] == "invalidArguments"
        addon = resources / "android-tools/runtime/network_addon.py"
        original = addon.read_bytes()
        try:
            addon.write_bytes(original + b"\n# altered installed component\n")
            damaged = subprocess.run(command, env=env, capture_output=True, text=True, timeout=10)
            assert damaged.returncode == 10
            assert json.loads(damaged.stdout)["error"]["code"] == "payloadIntegrity"
        finally:
            addon.write_bytes(original)
    print("Installed bundle Doctor passed with an empty PATH and rejected an altered component.")

if __name__ == "__main__":
    main()
