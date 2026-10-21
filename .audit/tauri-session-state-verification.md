# Tauri missing session state verification

Issue [15](https://github.com/raymondctc/mobile-capture/issues/15) tracks the failure after a completed session directory is deleted while its app stays open.

## Cause and change

Start previously read the old session files before releasing its GUI lease. Missing files prevented release, so the completed guardian retained ownership and blocked the next capture.

The client now retains a private authenticated completion token for its own session. A new Start can use that token when the session state is missing, but must still wait for the actual owner lease. Offline state cannot create the token. Active or unverified sessions stay blocked. Snapshot and Export identify the missing directory. A sealed guardian exits on GUI lease closure without writing a replacement view into the deleted directory.

## Regression evidence

The test-only commit precedes the fix. The moved-directory test failed with `Err("Private state is missing")`, with 0 passed and 1 failed.

The focused sequential network suite passed 26 tests. The additional launch-failure retry test passed separately, for 27 distinct passing tests. The full crash-boundary matrix was excluded from this local run. The tests cover completed evidence preservation, active and recovery-required refusal, offline-only refusal, competing clients, and retry after a launch failure.

Core strict Clippy, workspace formatting, and diff checks passed. The Windows core and test targets passed `cargo check --all-targets --target x86_64-pc-windows-gnu` with the explicit Rustup 1.99 toolchain. Native Windows execution remains for CI.

## Native app evidence

The macOS app was rebuilt at `TauriPoC/target/release/bundle/macos/Mobile Capture PoC.app`. A separate fixture bundle used compiled fake ADB, a fake proxy engine, and a private routing namespace.

The old native app reproduced the generic missing-state error and blocked Start after a stopped fixture session directory was moved. The rebuilt app displayed a completed-files-missing message and then entered Recording for a new session. That replacement session stopped and exported successfully. Direct checks confirmed restored fake routing, an absent original directory, unchanged bytes in the moved archive, and an export without private runtime files.

A separate native disconnect check on the old app required recovery, restored routing after reconnect and retry, and allowed a subsequent capture. It distinguished device recovery from the deleted-directory defect.

No real phone capture, routing change, or certificate installation was performed. The rebuilt user app reopened with Pixel 4a listed.

## Review

An independent correctness and security review found no actionable findings. An independent no-comments review found no comments, suppressions, or unjustified workarounds. Both reviewed the exact source tree integrated into this branch.

The combined case of an old completed missing session followed by a published startup-reply failure was reviewed in source, with each half covered independently. It has no dedicated combined regression test.
