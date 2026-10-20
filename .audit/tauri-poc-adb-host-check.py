#!/usr/bin/env python3
"""Check explicit-host ADB refusal without a connected-device capture."""

import argparse
import json
from pathlib import Path
import socket
import subprocess


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--adb", required=True, type=Path)
    arguments = parser.parse_args()
    adb = arguments.adb.resolve(strict=True)
    with socket.socket() as reservation:
        reservation.bind(("127.0.0.1", 0))
        port = reservation.getsockname()[1]
    command = [str(adb), "-H", "127.0.0.1", "-P", str(port), "devices"]
    result = subprocess.run(command, capture_output=True, text=True, timeout=12)
    with socket.socket() as probe:
        probe.settimeout(0.5)
        listening = probe.connect_ex(("127.0.0.1", port)) == 0
    refused = "cannot start server on remote host" in result.stderr
    verified = result.returncode != 0 and refused and not listening
    print(json.dumps({
        "command": command,
        "exit_code": result.returncode,
        "server_listening": listening,
        "explicit_host_refusal": refused,
        "verified": verified,
        "stderr": result.stderr.strip(),
    }, indent=2))
    return 0 if verified else 1


if __name__ == "__main__":
    raise SystemExit(main())
