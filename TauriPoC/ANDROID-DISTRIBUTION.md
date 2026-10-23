# Build and check Mobile Capture Android

Mobile Capture Android is the Android-only Tauri desktop application. The existing Swift app, iPhone support, Sparkle feed, and release scripts remain separate. Initial package targets are Apple Silicon macOS and Windows x64. Windows installed-package acceptance runs in the dedicated Android desktop workflow.

## Development package

Install the project build prerequisites, run `npm ci` in `TauriPoC`, and use a new output directory.

```sh
npm run package:android -- --channel development --cache /absolute/dependency-cache --output /absolute/new-package
```

On Windows, use absolute Windows paths and run the equivalent command from a developer shell with Rust and the Windows build tools. The script builds the native worker and guardian, downloads only pinned complete archives, validates and stages the payload, compiles the manifest identity into Doctor and the application, and bundles an app or a current-user NSIS installer. A normal `tauri build` without this preparation is rejected.

macOS development packages are ad-hoc signed and unnotarized. Normal macOS approval still applies. The script signs nested payload files and bundles before writing their manifest, asks Tauri to retain those signatures, signs adjacent application executables, and signs the outer app last. It verifies the final app signature before its installed Doctor smoke test and computes checksums after the final ZIP exists.

Windows development installers are unsigned. Install with the normal installer UI or `/S /D=C:\absolute\new-installation` for the CI user-scope smoke test. The installation directory must be new and must not replace an app that owns an active or recoverable session.

The output includes the final archive or installer, `SHA256SUMS`, `BUILD.json`, the payload manifest, and the pinned dependency lock. `BUILD.json` distinguishes development and release channels and records whether the source checkout was dirty. The build script never publishes an artifact.

## Doctor

Doctor is a separate console executable. It launches no desktop window and takes no phone or routing ownership. On macOS run the installed executable as follows.

```sh
"/absolute/Mobile Capture Android.app/Contents/MacOS/android-doctor" --json --storage-dir /absolute/existing/private-directory
```

On Windows, run `android-doctor.exe` beside the installed application. `--sources` accepts `logs`, `network`, or `logs,network` and defaults to both. `--resource-dir` overrides only the absolute owning resource directory. It never selects external tools. Arguments must be unique and known. Invalid arguments still emit one JSON object and exit `64`.

The versioned JSON report contains `schemaVersion`, `command`, `scope`, `platform`, `checkedAt`, `hostTarget`, `hostPrerequisitesReady`, `sources`, `checks`, `deviceSetup`, and `error`. Check rows contain status, source, absolute path, and observed version where applicable. Exit `0` means the selected local prerequisites passed. Exit `10` requires action, including damaged or missing payloads. Exit `70` indicates an internal Doctor failure. Doctor checks the complete packaged manifest, executable architecture, worker, guardian, network addon, ADB version/help, mitmdump version/options for network sources, and a private writable storage directory with at least 512 MiB available. It removes its temporary storage probe.

Doctor never runs `adb devices`, `adb shell`, `adb reverse`, certificate transfer, or capture. USB debugging authorization, certificate installation, app trust, and fresh phone traffic remain explicit manual or separately requested setup actions. `deviceSetup.status` stays `notChecked` even when host checks pass.

`python scripts/smoke-installed.py --app /absolute/installed-app` drives the actual installed console binary with an empty PATH. It requires successful pinned-tool probes, strict JSON argument handling, and rejection of a temporarily altered addon. On Windows, pass the installed directory. The script restores the altered addon bytes before completion. Run it against a disposable package copy.

## Credential-gated release preparation

Use `--channel release` from a clean source checkout with the intended publisher credentials. On macOS, set `APPLE_SIGNING_IDENTITY` to an available Developer ID Application identity and `APPLE_NOTARY_KEYCHAIN_PROFILE` to an existing notarytool profile. The script uses hardened runtime signing, submits the final app for notarization, waits, staples, verifies, and then creates the final ZIP and hashes. It refuses missing credentials before building.

On Windows, set `WINDOWS_CERTIFICATE_THUMBPRINT` to an accessible code-signing certificate and ensure `signtool` is available. The script signs the owned runtime binaries before payload hashing. Tauri signs the application and sidecars during NSIS preparation. The final installer is timestamped and verified before checksums are written. Credentials are never stored in the repository or output metadata.

The manually dispatched `android-desktop-release.yml` workflow uses the `android-release` environment and dedicated Android publisher secrets. It imports credentials into the temporary runner, prepares signed artifacts, cleans the credentials, and uploads artifacts. It does not create or publish a GitHub release.

Publisher signing and notarization have not been exercised without publisher credentials. Native Windows CI, a clean Mac, Gatekeeper acceptance of a signed release, and physical Android setup are distinct acceptance checks. A passing fixture or local development Doctor cannot establish them.
