#!/usr/bin/env python3
"""Build a complete Android desktop bundle without publishing it."""
import argparse
import json
import os
from pathlib import Path
import platform
import shutil
import subprocess
import sys

from payload import ROOT, digest, prepare


def run(command, env=None):
    subprocess.run(command, cwd=ROOT, env=env, check=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--channel", choices=("development", "release"), required=True)
    parser.add_argument("--cache", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    args = parser.parse_args()
    windows = sys.platform == "win32"
    if windows and platform.machine().lower() not in ("amd64", "x86_64"):
        parser.error("Windows x64 is required")
    if not windows and (sys.platform != "darwin" or subprocess.check_output(["/usr/sbin/sysctl", "-n", "hw.optional.arm64"], text=True).strip() != "1"):
        parser.error("macOS Apple Silicon or Windows x64 is required")
    target = "x86_64-pc-windows-msvc" if windows else "aarch64-apple-darwin"
    source_dirty = bool(subprocess.check_output(["git", "status", "--porcelain"], cwd=ROOT, text=True).strip())
    if args.channel == "release" and source_dirty:
        parser.error("Release packaging requires a clean source checkout")
    output = args.output.absolute()
    if output.exists():
        parser.error("--output must be a new directory")
    identity = os.environ.get("APPLE_SIGNING_IDENTITY") if args.channel == "release" else "-"
    if args.channel == "release":
        if windows and not os.environ.get("WINDOWS_CERTIFICATE_THUMBPRINT"):
            parser.error("Release packaging requires WINDOWS_CERTIFICATE_THUMBPRINT and an accessible signing certificate")
        if not windows and (not identity or identity == "-" or not os.environ.get("APPLE_NOTARY_KEYCHAIN_PROFILE")):
            parser.error("Release packaging requires APPLE_SIGNING_IDENTITY and APPLE_NOTARY_KEYCHAIN_PROFILE")
    output.mkdir(parents=True)
    build_root = Path(os.environ.get("CARGO_TARGET_DIR", ROOT / "target")).absolute()
    binary_dir = build_root / target / "release"
    run(["cargo", "build", "-p", "capture-poc-core", "--bin", "collector-worker", "--bin", "capture-guardian", "--release", "--locked", "--target", target])
    generated = ROOT / "src-tauri/generated" / (target + "-" + args.channel)
    if generated.exists():
        shutil.rmtree(generated)
    generated.mkdir(parents=True)
    payload = generated / "android-tools"
    manifest_digest = prepare(target, args.cache.absolute(), payload, binary_dir, None if windows else identity)
    env = os.environ.copy()
    env["MOBILE_CAPTURE_PAYLOAD_SHA256"] = manifest_digest
    env["MOBILE_CAPTURE_PACKAGE_BUILD"] = "1"
    env.pop("APPLE_SIGNING_IDENTITY", None)
    run(["cargo", "build", "-p", "capture-poc-core", "--bin", "android-doctor", "--release", "--locked", "--target", target], env)
    suffix = ".exe" if windows else ""
    binaries = ROOT / "src-tauri/binaries"
    binaries.mkdir(exist_ok=True)
    for name in ("collector-worker", "capture-guardian", "android-doctor"):
        shutil.copy2(binary_dir / (name + suffix), binaries / (name + "-" + target + suffix))
    resources = {str(payload): "android-tools/", str(ROOT / "resources/network_addon.py"): "network_addon.py"}
    overlay = generated / "tauri.payload.conf.json"
    configuration = {"build": {"beforeBuildCommand": "node scripts/require-package.mjs && npm run web:build"}, "bundle": {"resources": resources}}
    if windows and args.channel == "release":
        configuration["bundle"]["windows"] = {"certificateThumbprint": os.environ["WINDOWS_CERTIFICATE_THUMBPRINT"], "digestAlgorithm": "sha256", "timestampUrl": "http://timestamp.digicert.com"}
    overlay.write_text(json.dumps(configuration, indent=2) + "\n")
    cli = ROOT / "node_modules/@tauri-apps/cli/tauri.js"
    command = ["node", str(cli), "build", "--target", target, "--bundles", "nsis" if windows else "app", "--config", str(overlay), "--ci"]
    if not windows or args.channel == "development":
        command.append("--no-sign")
    run(command, env)
    bundle_root = binary_dir / "bundle"
    if windows:
        installers = list((bundle_root / "nsis").glob("*.exe"))
        if len(installers) != 1:
            raise RuntimeError("Expected exactly one Windows installer")
        if args.channel == "release":
            run(["signtool", "sign", "/sha1", os.environ["WINDOWS_CERTIFICATE_THUMBPRINT"], "/fd", "SHA256", "/tr", "http://timestamp.digicert.com", "/td", "SHA256", str(installers[0])])
            run(["signtool", "verify", "/pa", str(installers[0])])
        shutil.copy2(installers[0], output / ("MobileCaptureAndroid-Windows-x64-" + args.channel + ".exe"))
    else:
        app = bundle_root / "macos/Mobile Capture Android.app"
        if not app.is_dir():
            raise RuntimeError("Tauri did not generate the expected app")
        installed_payload = app / "Contents/Resources/android-tools"
        shutil.rmtree(installed_payload)
        shutil.copytree(payload, installed_payload, symlinks=True)
        for executable in (app / "Contents/MacOS").iterdir():
            if executable.is_file():
                command = ["/usr/bin/codesign", "--force", "--sign", identity]
                if args.channel == "release":
                    command += ["--options", "runtime", "--timestamp"]
                run(command + [str(executable)])
        command = ["/usr/bin/codesign", "--force", "--sign", identity]
        if args.channel == "release":
            command += ["--options", "runtime", "--timestamp"]
        run(command + [str(app)])
        run(["/usr/bin/codesign", "--verify", "--deep", "--strict", str(app)])
        if args.channel == "release":
            notarize_zip = generated / "notarize.zip"
            run(["/usr/bin/ditto", "-c", "-k", "--keepParent", str(app), str(notarize_zip)])
            run(["/usr/bin/xcrun", "notarytool", "submit", str(notarize_zip), "--keychain-profile", os.environ["APPLE_NOTARY_KEYCHAIN_PROFILE"], "--wait"])
            run(["/usr/bin/xcrun", "stapler", "staple", str(app)])
            run(["/usr/bin/xcrun", "stapler", "validate", str(app)])
        installed = output / app.name
        shutil.copytree(app, installed, symlinks=True)
        run([sys.executable, str(ROOT / "scripts/smoke-installed.py"), "--app", str(installed)])
        archive = output / ("MobileCaptureAndroid-macOS-arm64-" + args.channel + ".zip")
        run(["/usr/bin/ditto", "-c", "-k", "--keepParent", str(installed), str(archive)])
    shutil.copy2(payload / "payload.json", output / "payload.json")
    shutil.copy2(ROOT / "dependencies/downloads.lock.json", output / "downloads.lock.json")
    (output / "BUILD.json").write_text(json.dumps({"schemaVersion": 1, "productName": "Mobile Capture Android", "identifier": "dev.mobilecapture.tauri-poc", "target": target, "channel": args.channel, "payloadSHA256": manifest_digest, "commit": subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip(), "sourceDirty": source_dirty}, indent=2) + "\n")
    files = sorted(path for path in output.iterdir() if path.is_file())
    (output / "SHA256SUMS").write_text("".join(digest(path) + "  " + path.name + "\n" for path in files))
    print(output)

if __name__ == "__main__":
    main()
