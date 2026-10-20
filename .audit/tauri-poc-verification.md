# Tauri PoC verification

## Scope

The PoC is an isolated Android whole-device log capture app. Fixture runs use a compiled fake ADB executable with the real owner, worker, timeline normalizer, exporter, and native Tauri interface. They do not establish phone hardware acceptance.

## Existing application baseline

Before implementation, `bash Scripts/test-all.sh python` passed. The capture suite ran 68 tests with one optional case skipped. The release suite ran 75 tests with two optional cases skipped. The local output is `/private/tmp/mobile-capture-tauri-baseline.log`.

## Independent exporter check

`.audit/tauri-poc-check-archive.py` imports the existing Python normalizer. It checks original raw bytes and file presence, stable event IDs, line references, checksums, source counts, schema 1, and the allowed archive files. Its self-test accepts a canonical Python package and rejects five seeded faults.

```sh
python3 -B .audit/tauri-poc-check-archive.py --self-test
python3 -B .audit/tauri-poc-check-archive.py --session SESSION --archive ARCHIVE
```

The committed explicit-host ADB probe also ran against the installed Android platform-tools. With an unused loopback port, ADB returned exit 1 and did not start a server. The result is `tauri-poc-adb-host-check.json`. No phone was captured.

## Native macOS acceptance before review fixes

The coordinator launched the packaged app built from the implementation commits represented by delivery SHA `b253c5a`. The fixture banner was visible. Start was disabled until explicit whole-device consent. The app discovered a fixture device, recorded live rows, added a marker, displayed event details, stopped, and exported through its native controls.

The independent exporter check accepted 88 logs and one marker. The fixture contains malformed and invalid UTF-8 records, so the package correctly reported partial evidence. The machine-readable result is `tauri-poc-native-export-check.json`.

After Stop, the owned fake ADB process 51449 no longer existed. A second recording was ended with normal app Quit. The launcher returned exit 0, the worker receipt recorded `child_reaped=true`, and the owned fake ADB process 53611 no longer existed. These PIDs were used only for read-only liveness checks.

## Independent review

The comments reviewer found two redundant comments and one justified Windows semantics comment. The redundant comments were removed. The Windows comment remains.

The correctness reviewer reproduced three defects at `b253c5a`. An abnormal collector exit racing Stop could produce a complete package. Export omitted an existing empty marker file and hid a missing marker path. An unterminated startup configuration could start collection. The review blocked publication pending corrections and regression checks.

Delivery commit `fce91cf` corrects all three paths without changing the public API or receipt schema. The correction owner demonstrated the new tests failing on the original implementation. The coordinator reran `npm run verify` after integration. All 17 behavioral tests passed, and the six Rust-exported fixture events matched the existing Python normalizer. `cargo fmt --all -- --check` and `cargo clippy -p capture-poc-core --all-targets --locked -- -D warnings` also passed. The verification output is `/private/tmp/mobile-capture-tauri-root-final-verify.log`.

New cases cover an abnormal exit code and unexpected signal during Stop, clean intentional SIGTERM, incomplete startup frames, an existing empty marker file, and refusal to hide a missing marker file. The correction owner also repeated the original paused-worker race with process-state gates. Its receipt now records `collector_exit` and exit code 23. The artifacts are `/private/tmp/mobile-capture-tauri-fix-validation-7kh4cusu`.

## Corrected native macOS acceptance

The coordinator built the packaged app from `fce91cf` after formatting and drove it with the compiled fixture. A collector configured to exit 23 on Stop produced the visible state `Collector ended · partial capture`. Export with zero markers passed the independent checker with 100 log events and the empty raw marker file included. Its receipt records `collector_exit`, exit code 23, `child_reaped=true`, and no forced termination. Results are `tauri-poc-native-correction-check.json` and `tauri-poc-native-correction-receipts.json`.

A second recording used the clean-stop fixture. Normal Quit returned exit 0 and wrote a lease-closed receipt with 100 records, verified reaping, and no forced termination. No fake ADB process from this native fixture remained. The local native-build log is `/private/tmp/mobile-capture-tauri-root-native-build.log`.

## Cross-platform CI

The initial [CI run](https://github.com/raymondctc/mobile-capture/actions/runs/39465423462) passed all macOS checks and produced the app bundle. Windows passed formatting, Clippy, and all 15 Rust tests, including the actual collector-process wait after owner Job closure. Its Python comparison helper then failed because the existing normalizer's file reader uses POSIX directory descriptors.

Commit `b1e92ae` supplies the ZIP's manifest and raw byte mapping to the same existing Python normalization function. The old and new helpers produced identical results on the same archive under independent review. The coordinator reran `npm run verify` successfully. The shipping Python runtime remains unchanged. The [next CI run](https://github.com/raymondctc/mobile-capture/actions/runs/39466186424) passed all checks and produced both the macOS app and Windows NSIS installer.

The independent correction reviewer confirmed the first three fixes but reproduced a new Unix process-group regression. When the ADB leader exited before its descendants, the guard added in `fce91cf` skipped the group signal and left a descendant holding its pipes. The reviewer compared both compiled workers and stopped all probe processes. PR readiness was blocked until this regression could be corrected and independently rechecked.

Commit `6231276` observes status without reaping and signals the group before releasing its identifier. Its compiled regression covers exited leaders with status 0, status 23, and SIGUSR1. The coordinator reran all 18 tests, formatting, Clippy, and the native build. Native capture, marker, Stop, and export passed again with 208 logs and one marker in `tauri-poc-native-final-check.json`. The actual collector PID 93137 exited. The fixture launcher returned exit 0 after Quit. A normal-mode window was then closed without consent or capture.

Final independent review reproduced a remaining gap at `6231276`. A SIGTERM-ignoring descendant retained a pipe after the leader exited. The worker's leader-only wait skipped escalation, then its unbounded drain could not finish. Both worker and descendant were still alive with no receipt 4.5 seconds after lease closure. The reviewer stopped both processes. This disproved the leader-only shutdown premise and blocked PR readiness despite the passing responsive-descendant and native scenarios. The whole-group and pipe-completion deadline correction below addressed that gap.

Delivery commits `418853f` and `5199ab3` add the failing cases and replace the leader-only wait. Both pipe readers and the unreaped leader share the one-second grace period. SIGKILL escalation still uses the owned group before reaping. A second one-second deadline returns an error without a receipt if completion remains unknown. Stderr keeps its 8 KiB diagnostic bound while reading through EOF.

The coordinator reran `npm run verify`, formatting, and Clippy with warnings denied. All 21 tests passed. Four group tests cover 12 scenarios across leader exit 0, exit 23, and SIGUSR1. They include responsive descendants, SIGTERM-resistant descendants, stderr-only inherited pipes, and a full stderr diagnostic buffer. The original ignoring-descendant probe now records forced termination and verified reaping with the descendant absent. The author's escaped-group stderr probe exits 1 with no receipt after the second deadline.

Independent GPT Sol review of exact delivery HEAD `1f00f56` returned CLEAN. The reviewer extracted that commit into a separate directory and reran all four group tests. The result is `tauri-poc-code-review.md`. The comment gate also passed with zero deletion candidates and zero MUST KILL flags.

The coordinator rebuilt the packaged app from `1f00f56` and drove explicit consent, capture, a marker, Stop, and export. The independent ZIP checker accepted 84 logs and one marker, including original raw bytes and checksums. `tauri-poc-native-bounded-check.json` records the result. The fixture deliberately emits malformed text, so partial evidence is expected. The actual collector PID 19014 no longer existed after Stop. Closing the app returned exit 0 and left no process for the tested application binary. No hardware capture occurred.

The [final-code CI run](https://github.com/raymondctc/mobile-capture/actions/runs/39468978227) at `1f00f56298c3d2379e3db73202cc433801aad70a` passed both jobs. macOS ran all 21 tests and built the app bundle. Windows ran all 15 applicable tests, the Python oracle comparison, formatting, and Clippy, then built and uploaded the NSIS installer. `tauri-poc-ci-check.json` records both successful job outcomes. Later closure changes are confined to audit artifacts.

The committed file scope is limited to `TauriPoC/`, its audit artifacts, and the new isolated workflow. The shipping Swift sources, production Python runtime, and existing release configuration have no diff from `origin/main`.

The GPT Luna audit reviewer compared the decision trail with this task's transcript through `2026-10-20T17:52:40Z` and found no mismatches. Its report is `tauri-poc-audit-review.md`. PR creation follows this completed code and audit checkpoint.

## Open acceptance

No Windows desktop UI or phone hardware run has been observed. macOS worker SIGKILL can leave the ADB client running. Windows forced process termination remains partial. Network capture, app scope, iPhone, and production recovery are outside this PoC. These limits remain separate from the completed PoC acceptance above.
