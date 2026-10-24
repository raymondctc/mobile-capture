# Android desktop port

## Outcome

An Android user can install the Tauri application on macOS or Windows, complete local diagnostics and manual certificate setup, select a device and an installed app, capture logs, network traffic, or both, stop safely, inspect and search the complete saved evidence, and export it. The existing Swift application keeps its iPhone implementation and release pipeline.

Completion requires a packaged macOS run through the fixture workflow, Windows CI tests and installer generation, and independent review of app identity, routing restoration, saved evidence admission, and dependency packaging. Physical-device trust confirmation and publisher signing cannot be inferred from fixtures or CI.

## Units

1. Preserve the current PoC behavior in an isolated branch, merge current main, capture the fake-device baseline, and write an Android parity inventory.
2. Agree on typed capture options and a read-only evidence library. Establish verification for the missing capabilities before implementing them.
3. Port Android app discovery and identity-verified logging. Support independent sources and configurable bounded capture policies. Preserve guardian restoration and evidence sealing.
4. Add saved-session listing, reopening, full timeline queries, detail and body inspection, and export destination handling. Keep private runtime state outside evidence browsing.
5. Add setup diagnostics, certificate handoff, an owned HTTPS check, and readiness reporting. Keep device-side certificate trust explicit.
6. Build the Android desktop interface around those APIs. Add source and scope selection, settings, saved sessions, full search, timeline reading controls, setup, and diagnostics.
7. Package pinned ADB and mitmdump payloads with licenses for macOS and Windows. Verify installed resource selection, macOS app generation, and Windows installer generation.
8. Review, run the whole workflow, document supported behavior and limits, and open a reviewable PR above the existing PoC stack.

## Rigor and effort

This is a multi-hour migration across seven implementation units after grounding. Identity, restoration, and evidence admission receive the strongest review because errors can collect unrelated logs, leave a phone routed through a stopped proxy, or misstate evidence. UI and documentation changes use build checks and native fixture verification. Writers use separate worktrees and merge only after their unit checks pass.

## Baseline

The implementation branch starts at the latest PoC fix, 73e12102ec87e602a4c8684ddb91bba46de3454c, and merges main 18c47f110a817eaedb67eb858cff9e9ce3c77b68. Its initial head is 941074c9da5379818d0a30509b6d206f29742f9b.

The first local test run could not use loopback sockets inside the sandbox. That run is inconclusive. The host-permitted run is recorded separately at `/private/tmp/android-port-baseline-verify-host.log`.

Issue: https://github.com/raymondctc/mobile-capture/issues/19.
