# Run the Tauri Android capture PoC

This isolated desktop app records real whole-device Android logcat output after explicit consent. The timeline shows the latest 500 events. Export includes every admitted raw record, normalized events, markers, context, and a schema 1 manifest with checksums.

App scope, network capture, iPhone capture, routing changes, certificate installation, production recovery, and updates are unavailable. This app does not replace the Swift application.

## Run locally

Install Node 22, Rust, and the platform's Tauri build prerequisites. Install Android platform-tools and put `adb` on `PATH` for normal device discovery. On this Mac, Node 22 is at `/opt/homebrew/opt/node@22/bin`.

```sh
cd TauriPoC
export PATH=/opt/homebrew/opt/node@22/bin:$PATH
npm ci
npm run dev
```

On Windows PowerShell, after installing the platform build prerequisites:

```powershell
cd TauriPoC
npm ci
npm run dev
```

Connect an Android device or start an emulator. Enable USB debugging and accept the device authorization prompt. Refresh devices, select a ready device, check the whole-device consent box, and choose **Start capture**.

Add a marker to record a reproduction step. Select a timeline row to inspect its stable ID, raw reference, parsed fields, and clock basis. Search filters only the displayed recent window.

Choose **Stop** to close the worker lease and wait for the collector receipt. **Export evidence ZIP** becomes available after verified shutdown, including verified failed or disconnected captures. The saved local path appears below the export button. Review raw evidence before sharing it.

Normal sessions use the Tauri application's data directory under `dev.mobilecapture.tauri-poc`, with a separate `sessions` directory. They do not use the production app's private state.

## Verify without a phone

```sh
npm run verify
```

The command compiles a fake ADB executable and the real worker, runs the Rust behavioral tests, generates a ZIP through the real owner, and compares its events against `CaptureRuntime.evidence`. Python 3 is required. No device, routing, or trust setting changes.

Launch the native development app with a clearly labelled fixture device:

```sh
npm run fixture
```

The fixture launcher alone sets the explicit fixture environment, fake-ADB path, and a fresh temporary session root. Starting the app normally uses real ADB. The fixture emits malformed text and invalid UTF-8 deliberately, so its ZIP reports partial evidence.

## Build and check the packaged app

On macOS:

```sh
npm run build -- --bundles app
npm run fixture -- "$PWD/target/release/bundle/macos/Mobile Capture PoC.app/Contents/MacOS/mobile-capture-tauri-poc"
```

On Windows PowerShell:

```powershell
npm run build -- --bundles nsis
npm run fixture -- "$PWD/target/release/mobile-capture-tauri-poc.exe"
```

The build compiles `collector-worker` and packages it as a Tauri external binary. The built-app fixture launcher uses that packaged worker beside the app executable. The app exposes six Rust commands and no frontend shell access. The separate `tauri-poc.yml` workflow runs fixtures and builds on macOS and Windows. Windows CI evidence is separate from Windows phone acceptance.

## Capture and shutdown limits

Raw admission stops at 32 MiB, 100,000 newline-terminated records, or 120 seconds. Earlier evidence is retained. Markers are limited to 4096 UTF-8 bytes each and approximately 1 MiB in total. The UI polls sequentially and returns at most 500 recent normalized events. This PoC rereads its bounded raw files and has no production SQLite timeline index.

The embedded Rust owner retains the worker process and its only stdin lease writer. The worker receives startup configuration before it can launch logcat. On Windows the owner assigns the waiting worker to a kill-on-close Job Object before sending configuration. Assignment failure refuses capture. A normal owner exit closes the lease. Abrupt owner exit closes the Windows Job, which can kill the worker before it writes a receipt.

The worker uses `adb -H 127.0.0.1 -P 5037` for collection. Discovery may prepare the normal ADB server outside the worker Job. The capture never kills that shared server.

On macOS the worker owns the ADB process group and sends SIGTERM. It gives the leader and both output pipes one second to finish, then escalates the group to SIGKILL. A second one-second deadline refuses a receipt if completion remains unverified. The leader stays unreaped until group signaling ends, so its identifier cannot be reused during escalation. Stderr retains at most 8 KiB while its reader continues through EOF. The worker waits for its ADB child before writing the receipt. On Windows stopping the owned ADB client uses process termination, which is recorded as forced and partial. The owner waits up to four seconds for its worker. Missing or failed receipts produce **Shutdown unverified** and disable export.

A worker killed with SIGKILL on macOS can leave its ADB client running until that client exits. No persisted PID authorizes termination. A hard-killed worker cannot establish verified shutdown. This PoC has no recovery adapter and makes no routing changes.

## Evidence contracts

Raw logs remain byte-for-byte, including CRLF, invalid UTF-8, malformed records, and partial tails. Event IDs hash the source, relative path, one-based original line number, and original record bytes using the existing schema 1 algorithm. Device clock uncertainty remains unknown. Local markers use host time with nominal 1 ms uncertainty.

Export admits only this PoC's `logs/device.log` and `timeline/markers.jsonl`. It rejects symlink, reparse, and nonregular admitted paths. It creates the archive privately, hashes the included bytes, and publishes the ZIP atomically. Runtime metadata and worker receipts are excluded. Limits, disconnects, forced termination, incomplete tails, empty captures, and malformed records remain partial in the manifest. Repeated exports of unchanged evidence are byte-identical.
