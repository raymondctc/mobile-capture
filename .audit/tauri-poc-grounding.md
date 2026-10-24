# Tauri PoC grounding

Issue https://github.com/raymondctc/mobile-capture/issues/7 tracks this work. The delivery branch is `codex/tauri-poc`.

The existing production capture coordinator is Swift. The Python coordinator is legacy. New code must not replace the current application or share its private recovery state.

The native host owns its collectors through process handles and keepalive pipes. Persisted PIDs never authorize a kill. Routing is restored while the proxy is still alive. Recovery journals precede every device mutation. Android proxy restoration includes cache acknowledgement and exact restoration of all five saved fields. A UI framework does not provide these guarantees.

The smallest useful and safe PoC is Android log capture with explicit scope, bounded admission, a real timeline, stop, and a schema 1 evidence ZIP. Live network capture remains unavailable until fixture tests establish recovery. The interface must say this plainly. Whole-device capture requires an explicit opt-in. Do not introduce an incomplete app-scope implementation that can silently collect other apps.

Key source records are `docs/CAPTURE-NATIVE-RUNTIME.md`, `docs/CAPTURE-PACKAGE.md`, `Sources/Core/CaptureLifecycle/NativeCaptureHost.swift`, `Sources/Core/CaptureLifecycle/CaptureCollectorWorker.swift`, `Sources/Core/CaptureServices/CaptureEvidenceExporter.swift`, and `Sources/Core/CaptureServices/CaptureLogDiscovery.swift`.

Portable archives use `format: device-capture-evidence` and `schema_version: 1`. Raw admitted evidence remains byte-for-byte. Stable event IDs derive from source, path, one-based line number, and raw record bytes. Raw references use relative paths with one-based `#L` lines. The manifest reports gaps and partial outcomes. It must never infer completeness from successful ZIP creation. Exclude private state and arbitrary files. Reject symlinks and nonregular evidence paths.

The raw allowlist is `logs/device.log`, `network/events.jsonl`, `network/capture.har`, `network/capture.mitm`, `network/incomplete.json`, and `timeline/markers.jsonl`. Logs-only captures report network as unselected. Live session metadata uses a separate PoC coordinator identity and storage root. Production session recovery compatibility is out of scope.

The read-only how explainer recommended this slice after tracing process ownership, app filtering, routing, and export. Windows build evidence is distinct from Windows device acceptance. The current host is macOS on Apple Silicon. Node 22.22.2 is available at `/opt/homebrew/opt/node@22/bin/node`; prepend that directory for npm and frontend builds.

Current Tauri documentation was fetched with Context7. References are https://v2.tauri.app/develop/state-management/, https://v2.tauri.app/develop/calling-rust/, https://v2.tauri.app/develop/sidecar/, and https://v2.tauri.app/security/capabilities/. Use Rust commands for the narrow application API. No frontend shell access is needed. A packaged helper needs an executable for each target platform. The coordinator owns the three-command Context7 budget, which is exhausted for this implementation question.

The design experiment must compare at least two complete ownership shapes. A separate Rust helper with a JSONL control protocol and an embedded Rust owner are both candidates. Explain parent death, normal close, stop idempotency, collector disconnect, bounded storage, and export behavior. Keep the useful PoC small enough to prove on the available machine.
