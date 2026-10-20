# Run the Tauri Android capture PoC

This isolated desktop app captures whole-device Android logs after explicit consent. Network capture is optional and requires separate consent. The timeline displays the latest 500 events. The evidence ZIP contains admitted raw sources, normalized events, markers, context, coverage gaps, and a schema 1 manifest with checksums.

App scope and iPhone capture are unavailable. This app does not replace the Swift application.

## Prerequisites and local run

Install Node 22, Rust, the platform's Tauri build prerequisites, and Android platform-tools. Put `adb` on `PATH`. Network mode also requires an installed **mitmdump 12.2.3**. The proxy engine is not bundled. Set `MOBILE_CAPTURE_POC_MITMDUMP` to its absolute executable path if it is not discoverable through the normal installation. The app packages its own PoC addon.

```sh
cd TauriPoC
export PATH=/opt/homebrew/opt/node@22/bin:$PATH
npm ci
npm run dev
```

The PATH line above is specific to the development Mac. On Windows PowerShell, run `npm ci` and `npm run dev` after installing the same prerequisites.

Connect an Android device or start an emulator. Enable USB debugging and accept the authorization prompt. Refresh devices, select a ready device, and check the whole-device logs consent box. To capture network traffic, also check the separate network consent box. Starting network mode configures temporary device proxy routing. The app shows the session certificate path and synthetic check URL supplied by the running engine. Install and trust that certificate on the Android device manually. The app does not alter host trust settings. HTTPS content whose trust or decryption fails is not observed as decoded HTTP.

Add a marker to record a reproduction step. Search and source filtering cover the displayed recent window. Selecting a network event shows decoded details and at most 1024 original body bytes as a labelled preview. Binary previews appear as base64. Admitted complete bytes may be retained in the ZIP; unavailable bodies and coverage gaps appear in the manifest. A TLS error count does not mean HTTP content was observed.

Choose **Stop** to restore device routing and wait for source shutdown. If recovery is required, keep the device connected and choose **Retry recovery**. Export and a new capture stay disabled until restoration and source verification finish. A verified partial capture can be exported, with its limitations recorded in the manifest. Review raw evidence before sharing it.

Normal sessions use the Tauri application's data directory under `dev.mobilecapture.tauri-poc`. Private runtime state is separate from exported evidence. A guardian process owns network session restoration after a GUI interruption. A hard guardian death can interrupt capture; recovery then reports partial evidence. No saved process ID authorizes killing an unrelated process.

## Verify without a phone

```sh
npm run verify
npm run fixture
```

The verification command builds the fake ADB and real collector, exercises the Rust owner, and checks an exported ZIP against `CaptureRuntime.evidence`. The fixture launcher compiles fake ADB and starts the native development app with fresh private output and runtime directories. Its banner identifies fake ADB. Selecting network mode uses the installed real proxy engine but does not capture phone traffic.

The separate `scripts/probe-network-engine.py` check requires `CAPTURE_TEST_MITMDUMP` set to an absolute path to mitmdump 12.2.3. It uses loopback HTTP and HTTPS with private client trust, then compares original body bytes across native flows, HAR, and JSONL. CI runs this real-engine check on macOS and Windows.

## Build and check the packaged app

On macOS:

```sh
npm run build -- --bundles app
npm run fixture -- "$PWD/target/release/bundle/macos/Mobile Capture PoC.app/Contents/MacOS/mobile-capture-tauri-poc"
```

On Windows PowerShell, build with `npm run build -- --bundles nsis` and launch the installed executable through the fixture script. The build packages `collector-worker`, `capture-guardian`, and `network_addon.py`. It does not package mitmdump. The Tauri adapter exposes capture commands without frontend shell access. The `tauri-poc.yml` workflow builds macOS app and Windows NSIS artifacts. A fixture and CI run do not establish physical-device acceptance.

## Capture and evidence limits

Raw log admission stops at 32 MiB, 100,000 newline-terminated records, or 120 seconds. Network admission is separately bounded to 32 HTTP flows and 120 seconds, with 256 KiB per retained request or response body, 16 KiB of headers per message, and a 96 MiB aggregate artifact budget. Reaching an evidence limit does not imply the proxy stopped forwarding while routing still points to it. Network omission counts, unavailable bodies, errors, forced stops, and interruptions make evidence partial.

Raw logs retain CRLF, invalid UTF-8, malformed records, and partial tails. Network evidence includes admitted native flow copies, HAR, event records, and an incomplete-flow summary. Export includes only the documented raw sources and excludes private runtime state, routing records, control credentials, and the private CA. Event IDs derive from raw source, relative path, one-based line number, and original record bytes. Device clock uncertainty remains unknown; local markers use host time. The manifest is authoritative for source coverage, restoration, and completeness.

The worker uses `adb -H 127.0.0.1 -P 5037` for collection. Discovery may prepare the shared ADB server. Capture does not kill that server. The PoC changes only selected Android device routing in network mode and restores it before export. Device disconnection or external routing changes can require a recovery retry. Keep the original device available until recovery finishes.

Log-only shutdown still has platform limits. On macOS, a hard-killed worker can leave its ADB client running until that client exits; without a verified receipt, export stays disabled. On Windows, stopping the owned ADB client can require termination and is recorded as forced, partial evidence. Network recovery does not erase these source limitations.
