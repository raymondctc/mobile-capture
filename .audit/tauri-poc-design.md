# Tauri PoC design

## Problem

The PoC must prove a real portable capture path without replacing the Swift app or weakening its existing app-scope and proxy-recovery behavior. It covers explicit whole-device Android logs. Network capture and app scope remain unavailable.

## Usage

The user refreshes devices, selects an authorized Android device, acknowledges whole-device logging, and starts capture. The timeline shows real log rows and markers. Stop waits for the owned collector to exit. Export publishes one evidence ZIP after verified shutdown. Unsupported capture modes stay visibly unavailable.

The frontend calls six outcome-level operations through Tauri commands. The operations are discovery, start, snapshot or timeline, marker, stop, and export. A snapshot includes phase, selected device, counts, effective bounds, recent events, warnings, and the archive path when available. There is no frontend shell capability.

## Shape

`TauriPoC/` contains the isolated application. React and TypeScript own presentation. A Tauri-independent Rust library owns device discovery, session state, timeline normalization, markers, and export. A small headless Rust worker owns the actual ADB logcat process, bounded raw-log admission, and its final receipt. Tauri is a thin adapter to the library.

The central data shapes are a ready-device identifier, explicit whole-device consent, a session phase enum, a stop reason, a collector receipt, and a bounded timeline page. Phases distinguish idle, recording, stopping, stopped, failed, and unverified shutdown. Export requires a verified stopped state. Repeated Stop returns the existing outcome. Worker evidence and owner metadata have separate writers.

The app retains its worker process handle and the only write end of the worker's stdin lease. The worker starts ADB only after receiving validated startup configuration. EOF during startup prevents collection. Later EOF, a duration or byte limit, collector exit, or Stop all end in one child shutdown path. Only live owned handles authorize termination. No persisted PID grants authority.

On macOS the worker owns the ADB child and its process group, escalates shutdown on bounded deadlines, drains pipes, and waits. A worker killed with SIGKILL can leave shutdown unverified. The PoC documents that limitation and makes no routing changes.

On Windows use a kill-on-close Job Object with an explicit startup handshake so the worker cannot start ADB before containment is established. Stable Rust does not expose a suspended primary thread handle. Do not combine `CREATE_SUSPENDED` with an unavailable stable resume operation. A direct Win32 launch is an alternative only if its process and thread handles are both owned and tested. Assignment failure refuses collection and terminates only the owned worker.

ADB server ownership stays outside the capture worker. Worker clients use explicit `-H 127.0.0.1 -P 5037` addressing. An empirical check of the installed ADB against an unused loopback port returned exit 1 and `cannot start server on remote host`, with no listener created. Discovery can prepare the ordinary server outside the worker job. The capture never kills that shared server. Windows CI must separately exercise the portable process path.

The effective PoC log policy is bounded and included in evidence metadata. The default candidate policy is 32 MiB, 100,000 records, and 120 seconds. Keep only a bounded recent timeline in memory. Export reads all retained raw records. Unknown device clock uncertainty remains explicit. Whole-device scope must be clear in both the interface and manifest.

Export uses the existing schema 1 format. Stable IDs match the current source, relative path, one-based line number, and raw-byte hash. The full timeline, raw logs, markers, context, manifest, README, and SHA256SUMS are generated from the admitted evidence. Exclude runtime state. Reject symlink or reparse evidence paths and nonregular files. Stage privately, hash the staged bytes, and publish atomically. Disconnect, malformed records, partial tails, limits, and forced shutdown remain visible in completeness metadata.

## Synthesis decision

The GPT Sol 6.1 xhigh candidate is the base. It concentrates lifecycle and evidence decisions in one Rust owner while moving only collection into a worker. The GPT Sol candidate proposed a full engine helper with request correlation and event streaming. That protocol adds a second controller without improving the macOS worker-death limit.

The independent GPT Luna judge preferred the embedded-owner design. Its rubric scores were 10 of 10 for the base and 7 of 10 for the full helper, conditional on addressing Windows launch and ADB-server ownership. No feature grafts were needed.

The coordinator accepts a small packaged worker binary instead of same-binary Tauri re-execution. This makes headless fixtures straightforward and retains the selected ownership split. The implementation must not grow a full JSONL capture-controller protocol.

## Tradeoffs

The PoC accepts explicit whole-device scope instead of an incomplete app filter. It accepts a separate worker artifact to test process lifetime without loading a GUI. It accepts a logs-only first slice while network routing recovery remains a separate verification gate.

## Implementation reconciliation

The coordinator accepted the packaged-worker adaptation after the independent judge confirmed it preserves the base design. The Windows handshake and explicit-host ADB constraint are implementation gates. Any further contract change must update this file before the next unit uses it.

The coordinator accepted snapshots that reread the bounded raw file and return the latest 500 events instead of maintaining a second timeline index. Frontend polling must not overlap. Search applies to the displayed recent window. Export still includes every admitted record. This PoC does not claim the production SQLite index's large-session behavior.

## Verification

The code owner builds a compiled fake ADB and the verification command first. Scenarios include discovery states, rejected scope, raw-byte retention, live rows, markers, repeated Stop, disconnect, parent EOF, limits, failed publication, stable event IDs, hashes, and unsafe-path refusal. The coordinator independently drives the built desktop application. Windows fixture and build evidence is distinct from Windows phone acceptance.
