- [x] Ground in Swift app identity reader/filter and existing guardian restoration contracts.
- [x] Agree concrete descriptor and source-selection shapes with root and library owner.
- [x] Add cheap parser/filter privacy and identity-gap behavior tests before integration.
- [x] Implement bounded Android discovery, identity probes, and filtered raw persistence.
- [x] Integrate independent source selections, limits, descriptor, and network policy.
- [x] Reopen stopped logs using private completion proof and source lease.
- [x] Preserve and run capture, library and focused guardian recovery behavior checks.
- [x] Deslop and inspect actual ZIP and raw evidence.
- [x] Commit the validated collection unit.
- [ ] Independent privacy and correctness review belongs to the root integration review.

Throughput checkpoint. Parser/filter tests are the cheap privacy gate. Full subprocess tests run after those pass. No unrelated full crash matrix rerun unless routing changes.

Verification results.

- The library unit suite passed all 12 tests.
- The capture suite passed all 21 tests, including raw app ownership, identity gaps, saved log proof, and cancellation of an in-flight probe.
- The focused network suite passed all 32 tests serially. Three existing expensive crash sweeps were excluded because the root ran the complete baseline matrix.
- The Unix descendant suite passed all four tests with host permission. The sandbox run failed at Unix socket creation with EPERM and was inconclusive.
- Clippy passed for all targets with warnings denied. The diff whitespace check passed.
- A parallel network run hit existing five-second cold engine launch deadlines in eight fixtures. All eight passed in the serial run used by the repository runner.

Source admission uses active user, unique UID, install identity, and device-time fences. UID capability is probed. The fallback verifies PID lifetimes and excludes unknown PIDs. Identity loss discards unverified memory and ends logs with an explicit gap. Every selected source keeps its own admission limits. The user stops routing explicitly after network admission closes.

The public capture.json descriptor is included in the ZIP. Private runtime files remain excluded. Saved log export requires a private completion proof bound to raw files and a free source lease. Saved network selection checks the private bootstrap, routing journal, owner lease, and source leases. Export still reacquires the existing guardian seal. The saved network test removed ADB before selection and export and observed no device commands.

No dependency or Cargo manifest changes were required. The root owns the Tauri adapter, the network addon policy implementation, and the final independent review.
