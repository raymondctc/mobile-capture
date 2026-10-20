# Tauri network verification

## Before implementation

The log-only baseline at `25da77a` passed `npm run verify`: 21 Rust behavioral tests and six canonical events compared with the existing Python normalizer. This result does not cover network capture. The complete output is local at `/private/tmp/tauri-network-baseline.log`.

The installed real mitmdump reports 12.2.3, embedded Python 3.14.4, OpenSSL 3.5.5. No phone proxy, host proxy, reverse tunnel, or trust settings have been changed.

## Required artifact checks

| Boundary | Controlled observation required |
| --- | --- |
| Startup | Listener and public CA ready before routing; engine failure causes no phone mutation; any partial claim rolls back. |
| Ownership | Production-compatible Mac lock and Windows cross-process exclusion; parent closure restores before engine shutdown; failed restoration leaves the listener alive. |
| Journal | Exact absent, empty, literal null, and active prior values survive claim and restore. Intent precedes reverse and settings commands. |
| Recovery | Disconnect, external conflict, cached-clear timeout, interrupted command and stale owner converge through explicit retry. No saved PID grants termination authority. |
| Reverse | Refuse occupied endpoint; remove only the claimed endpoint and matching target after verified settings restoration. |
| Engine | Real loopback HTTP and TLS interception preserve admitted body bytes in JSONL, HAR and native flows. Private client trust is confined to the probe. |
| Evidence | Deterministic source/path/line/byte IDs match Python; exact hashes and allowlist; body/pending/error/limit gaps make the package partial; unsafe paths refuse export. |
| Desktop | Built native application starts, displays and inspects network events, handles Stop/retry and exports a clearly labelled controlled fixture. |
| Portability | Exact-code Windows and macOS fixture, real-engine and packaging CI. CI is separate from physical phone acceptance. |

Results are appended after the implementation runs. This table is a verification contract, not a report that the checks passed.

## Recovery stress-test status

The backend writer's early full crash sweeps were invalidated because binaries were rebuilt during the run. An isolated later-boundary run passed, but that does not establish the full matrix. At 19:02 UTC the writer is tracing guardian lease reads, reply errors and exit reasons, and preparing a full sweep using immutable copied binaries with recorded SHA256 hashes. No full immutable matrix result is claimed here. `Interrupted` and `TimedOut` reads are not EOF; that correction alone does not establish the cause of the observed attachment failure.

An independent private-snapshot reproduction later recorded `guardian_started` with recovery enabled, immediately followed by `exit_quiescent_no_clients`, before an attachment or lease read. The backend writer reproduced the same trace at `/var/folders/9d/_w0dsyv95ys3r_6hd99pcxch0000gp/T/.tmpedSHfT`. A completed recovery guardian was exiting before its first client attached. Terminal no-client exit is now gated on an accepted attachment. This explains the reproduced completed-session refusal; it does not retroactively validate the mixed-binary sweeps or establish that every earlier refusal had this cause. The immutable focused 16-run late-restore probe saw no false lease EOF.

The independent reviewer also reproduced a Unix guardian zombie after a completed session, and a post-restoration writer-lock refusal that could be retried through the native API but had no retry button. The backend owner is adding child reaping without tying guardian life to the GUI. The coordinator added a retry button for network `unverified_shutdown` while retaining the existing log-only behavior. Logs are `/private/tmp/tauri-network-correctness-zombie.log` and `/private/tmp/tauri-network-correctness-unverified.log`. Final stable-source review and runtime results remain pending.

The reviewer found that the official macOS executable reports `Mitmproxy: 12.2.3 binary`, while the Rust preflight accepted only `Mitmproxy: 12.2.3`. The coordinator independently confirmed that output. Earlier standalone engine probes bypassed the Rust launcher and did not cover this prerequisite check. A private reproduction corrected only that spelling check and rejected the private CA in the TLS client. With otherwise clean logs, the source completion and ZIP incorrectly said complete while recording a TLS error. `/private/tmp/tauri-network-correctness-real-errors.log` records that counterexample. The backend owner is correcting both source completion and the exporter error predicate. Neither counterexample changes host trust or device routing.

## Rust owner and real engine

The corrected probe now passes through the actual Rust owner, guardian, version preflight, Stop and export with installed mitmdump. The coordinator independently checked its three actual Rust ZIPs with `.audit/tauri-network-check-archive.py`, including offline native replay. All eleven allowed members, checksums, canonical identities, body bytes, coverage and explicit restoration agreed.

| Controlled case | Network events | Decoded HTTP events | Package | ZIP SHA256 |
| --- | ---: | ---: | --- | --- |
| Trusted HTTP and HTTPS | 4 | 4 | complete | `b7e60e5c4fbd5d046974674d45d16bad0eedffc0ee04a94c702610ed92dfb73c` |
| TLS client rejects private CA | 1 | 0 | partial | `62d1f51bc5b11d91b18838a4e785ed3c35fe2659d01a2d1e58cf807799b49fd3` |
| Refused upstream HTTP | 2 | 1 | partial | `441aa15c0e1e6ee2e0aa8cf4ddf10759cf59fb38113c5968bd31ed71dcc5355a` |

Each case also has 100 valid log events, zero markers, and zero unavailable bodies. The TLS case has no HAR/native HTTP flow; the HTTP error has one; the trusted case has two. This removes malformed fixture logs as a reason the error cases could appear partial. Artifacts and probe result are under `/var/folders/9d/_w0dsyv95ys3r_6hd99pcxch0000gp/T/tauri-bounded-engine-proof-ydnu2a4x/`. Routing changes were confined to the compiled fake ADB database. This is executable core/engine proof; native UI and Windows CI remain separate gates.

## Engine-control prototype

Before implementation, `/private/tmp/tauri-network-engine-probe.py` ran installed mitmdump 12.2.3 with the existing nonce-bound setup response and a private addon watcher. It served one HTTP request and one HTTPS CONNECT request on loopback. The client trusted only the generated public CA in its own `SSLContext`. `connection_strategy=lazy` and `upstream_cert=false` kept the synthetic responses inside the proxy.

Writing a private stop request caused the addon to call `ctx.master.shutdown()`. Its `done()` hook ran and the engine exited 0. The run produced four raw request/response events, two HAR entries and a 5,660-byte native file. This proves graceful control and real TLS interception for this controlled path. It does not prove native replay parity, routing recovery, or full application support.

The coordinator also replayed the native file with mitmdump in no-server read mode. Both replayed flow response bodies matched HAR and JSONL byte for byte. Output: `/private/tmp/tauri-network-prototype-native.jsonl`. This adds replay parity for the two small synthetic setup responses, not binary admission or limit coverage.

Result path: `/var/folders/9d/_w0dsyv95ys3r_6hd99pcxch0000gp/T/tauri-network-engine-proof-5qbgvpy1/result.json`. No phone or host proxy/trust settings changed.

## Independent archive checker

`.audit/tauri-network-check-archive.py` verifies network ZIP membership, hashes, canonical raw-line identities, counts, coverage, HAR bodies and optional offline native replay. It requires logs and markers, including an empty marker file, and all four network artifacts. It deliberately rejects partial packages missing a selected network artifact rather than claiming to validate that case.

The first checker draft incorrectly tried to parse logcat as JSON and required event-style byte fields in HAR. A fresh correction worker fixed both after coordinator review. A selfcheck uses the actual prototype HAR/native/JSONL files, real logcat text and an empty marker file with the existing Python exporter. The coordinator reran the corrected checker with native replay: 11 files, one log event, four decoded network events, two HAR/native flows, zero markers, matching hashes and body bytes. Deliberately altered HAR size/body, timeline identity, raw-line omission, missing markers and missing HAR were rejected by the correction worker. This is a checker selfcheck against the Python exporter, not yet proof of the Rust backend archive.

Selfcheck ZIP SHA256: `0ffb8088c850ec23bf3c66d58720db03f79118088925032339e504085834b4b7`.

The coordinator then tightened the checker to require explicit verified restoration, reject UI-only CA/check fields in public metadata, and read the Rust `network_completion` forced/interrupted facts. The older Python selfcheck has no `routing_restored` field and fails this stronger contract as expected. A derived checker-only fixture declares the source probe's unchanged routing and explicit completion facts, without altering any raw flow bytes. It passes native replay with SHA256 `34f8982fb0f7bec3d064027e87383319636e4cad557aaf3124e921637862f412`. Rehashed false-complete forced-stop, unverified-routing and UI-metadata variants are rejected. This fixture remains a checker selfcheck, not Rust or device recovery proof. Result: `/private/tmp/tauri-network-checker-result.json`.

## Legacy exporter follow-up

The coordinator independently reproduced the Python helper's missing-body-only gap with otherwise valid probe artifacts and a synthetic nonempty collector `missing_bodies` list. It returned `package_state=complete` with no pending flows. [Issue #10](https://github.com/raymondctc/mobile-capture/issues/10) tracks the existing helper separately. The Tauri Rust path must mark that gap partial; the shipping helper is unchanged by this task.

## Final backend and independent review

This section supersedes the pending backend statements above. Commits `cffe575` and `1c700bd` integrate the final backend and raw-header follow-up. The author's unchanged copied binaries passed all 144 routing journal, command and verification crash boundaries, 32 focused startup/recovery attachments, and all 45 Rust tests without skips. The full network suite took 621.68 seconds. Evidence and compiled SHA256 identities are in `/private/tmp/tauri-network-core-evidence/backend-verification.md`, `final-backend-tests.log` and `frozen-binary-hashes.json`.

The intermittent frame failure was reproduced independently with a compiled macOS socket probe. Accepted streams inherited nonblocking mode; an incomplete frame returned `WouldBlock` immediately despite the read timeout. Explicit blocking mode accepted the delayed remainder. The final guardian applies that setting before bounded frame reads. The earlier polling explanations and failed/mixed-binary matrices remain excluded.

Independent reviews are `/private/tmp/tauri-network-correctness-review.md`, `/private/tmp/tauri-network-final-delta-review.md` and `/private/tmp/tauri-network-last-boundary-review.md`. The last review records source hashes, a private-copy fragmented-frame regression, a complete real-engine run and 24 writer-close/lease fault cases. A separate no-comments review found no required deletions. The final addon and probe hashes are `733d2f1eda2f3b47fcb648b3e2bcddb64953458e1bf7786865e6c447df97c65e` and `cb174b2f788c2e55ea647e6f50e4a48869defeda46fbf2de0bfcf3554ea4f80e`.

The independent final real-engine result is `/var/folders/9d/_w0dsyv95ys3r_6hd99pcxch0000gp/T/tauri-bounded-engine-proof-u0h6uoql/result.json`. It exercises binary/gzip body parity, private TLS, original flow IDs, finite admission, oversized unavailable bodies, shutdown/EOF, source-writer failures and raw non-UTF8 request/response headers. The Rust raw-header ZIP is partial, restored, omitted=1 and decoded=0, with SHA256 `84fc6af1b7ff44332fd35090cb9a29974aa42fc4708fa2820965fe1d446c6f96`. Forwarding remains functional when retention is declined.

## Packaged native fixture observations

The coordinator drove the actual macOS app with CUA, a compiled persistent fake ADB and installed mitmdump. The private namespace is `/var/folders/9d/_w0dsyv95ys3r_6hd99pcxch0000gp/T/mobile-capture-poc-fixture-OTdrz5`. These are simulated device observations; the HTTP(S) proxy and TLS interception are real. Host trust and real Android routing were unchanged.

Session `tauri-network-Fr3u3e` showed the fixture banner, separate whole-device consent, disabled Start before consent, explicit policy limits, decoded HTTP(S), network filtering, UTF-8 response detail and a marker. Simulated disconnection made Stop require recovery and kept Start/Export disabled while the proxy still forwarded controlled traffic. Reconnection and Retry restored the fake settings and reverse map. GUI Export produced SHA256 `2ea5fd0d300b3d7984f498e2cc7dc7e89f2bdc46bb003731ac8672687279b131`. The independent checker and native replay confirmed 11 files, 787 log events, one marker, eight network events, four HAR/native flows and no missing bodies or pending flows. Fixture malformed logs make the overall package partial. A specific `ECONNREFUSED` check outside the sandbox proved the stopped proxy port closed. The rebuilt app reopened the same completed session and exported it successfully.

The next Start exposed a separate ownership handoff race. A fixed 50 ms sleep after releasing the GUI socket did not prove the old guardian had released the OS lock. It refused safely before routing mutation but discarded the previous session view. A bounded actual-lease handoff and prior-view preservation are undergoing a separate fix and independent review. Earlier complete-suite results do not cover that new fix.

Session `tauri-network-OHTMUA`, built from `b83e591`, forwarded controlled HTTP/TLS plus a raw `0xff` header. The UI reported four decoded events and one omitted flow. A controlled private source lock held after Stop produced `Shutdown unverified`, disabled Export and a visible Retry recovery button. The lock holder then released normally. CUA screenshot verification stopped because macOS was locked; the user was asked to unlock. Retry, export, the corrected host-clock label, final consecutive capture and log-only UI verification remain pending.

## CI compiler alignment

Runs [39484327224](https://github.com/raymondctc/mobile-capture/actions/runs/39484327224) and [39485571995](https://github.com/raymondctc/mobile-capture/actions/runs/39485571995) failed on macOS and Windows before tests at Rust 1.99 Clippy. The failures were a redundant formatting borrow and fixture byte-array spelling. The first was fixed in `b83e591`; the second is a mechanical follow-up. Neither run proves runtime behavior or packaging.

The coordinator installed named `1.99.0-aarch64-apple-darwin` with Clippy, rustfmt and Windows GNU standard libraries to match CI. The default toolchain remains unchanged. The backend author previously installed named 1.95 plus Windows std, and also added Windows std to the preexisting Rosetta stable toolchain during an initial attempt. Those host setup changes are disclosed rather than rolled back over user state. Windows type checking is distinct from Windows execution. Final macOS/Windows CI and actual Windows guardian Job behavior are pending.

## Integrated handoff and final engine run

`f33d84995dccce817f95b3d56e5ed352f2c9984d` integrates the handoff correction from `094fae2`. `Client::release` relinquishes this GUI socket and observes the real OS owner lease for at most two seconds. A true competing client refuses a new capture while retaining the prior directory, snapshot, export and locator. The next guardian still acquires the authoritative lease itself. No routing, protocol, UI or sealing change is involved.

The old delayed-release regression fails on unmodified `b83e591`. All three new handoff tests, two existing network regressions and sixteen log-only tests pass on the final fix. The author report is `/private/tmp/tauri-network-handoff-evidence/verification.md`. A fresh reviewer copied and hashed all 56 tracked PoC files and independently passed the three handoff process tests. `/private/tmp/tauri-network-handoff-review.md` records no actionable findings and matches the integrated three-file hashes. Sandboxed loopback failures in both runs were excluded. This proves the core process behavior on macOS; the locked-screen final UI continuation is still pending.

`c017414` applies the fixture byte-array spelling correction. Named Rust 1.99 formatting and all-target Clippy pass locally for macOS and the Windows GNU target. [Run 39487720680](https://github.com/raymondctc/mobile-capture/actions/runs/39487720680) tests exact integrated revision `f33d849`; it is in progress. The previous 45-test matrix predates the handoff correction and is not represented as an exact-revision rerun.

The coordinator built the integrated debug binaries and ran the entire real-engine probe successfully. `/private/tmp/tauri-network-root-final-engine.log` and `/var/folders/9d/_w0dsyv95ys3r_6hd99pcxch0000gp/T/tauri-bounded-engine-proof-vv6gxbz1/result.json` record final raw-byte/native/HAR identity, admission, shutdown, metadata and writer-fault checks. All five actual Rust owner/guardian cases restored fake routing. The success source is complete; TLS failure, HTTP failure, writer-flush failure and binary-header omission are partial. Final native and Windows runtime results remain separate gates.

The coordinator independently checked and replayed all five resulting ZIPs. `/private/tmp/tauri-network-final-archive-checks.json` preserves the captured checker results and archive hashes. Each has eleven allowed members with matching checksums, raw identities and native/HAR bodies. This check runs against the actual Rust exports, not reconstructed fixture packages.

## Windows test-oracle correction

The Windows job in run `39487720680` passed formatting, Clippy and its owner-lock locator/exclusion unit test, then failed the network canonical-parity unit test. The Python oracle attempted the shipping helper's Unix directory-open path and received `PermissionError`. It had not reached the guardian process suite. `/private/tmp/tauri-network-ci-39487720680-windows.log:769` records the actual failure; there is no evidence here of a guardian launch failure.

`42783f1` changes only that test to provide its exact fixture bytes through the existing Python `raw_files` argument. The Rust file reader and full canonical-event equality assertion stay in the comparison. Shipping Python and production Rust are unchanged. `/private/tmp/tauri-network-windows-oracle-review.md` approves the scoped change and records its source hash. The focused test, Rust 1.99 formatting, and host/Windows-target all-target Clippy pass locally. Final CI must execute this new test revision on Windows.

## Measured Windows guardian launch

The corrected Windows unit oracle passed in run `39488430726`. Four unit tests, fourteen log-only capture tests and one fixture test passed. Twenty of twenty-two network tests then failed at the explicit breakaway launch with access-denied error 5, before routing. This is a measured production launch restriction, not a type-check result or evidence of failed Android restoration.

The coordinator required a separate owned-handle lifetime experiment before accepting any launch fallback. `dc57352` adds `scripts/probe-windows-launch.py`; `03a1ef9` runs it only on Windows. [Run 39489663292](https://github.com/raymondctc/mobile-capture/actions/runs/39489663292) passed the experiment. `/private/tmp/tauri-network-ci-39489663292-windows.log:141` contains its actual JSON. The immediate launcher Job has zero limit flags and does not allow breakaway. The breakaway launch fails with error 5. Plain and detached/process-group children are outside the GUI-owned source Job and survive actual GUI process termination. The source child inside the GUI-owned kill-on-close Job dies. Cleanup completes through owned or duplicated process handles. No host Job policy changes, saved-PID termination, or external process escape occur.

The immediate Job query cannot describe every ancestor policy. The design now explicitly guarantees independence from the GUI process and app-owned worker Jobs, while external host/launcher containment termination remains the hard-guardian-death and partial-recovery boundary. `1bcc69b` integrates the production correction from `76e4ccd`. It prefers breakaway and retries detached/group only after error 5, successful current-Job membership and limit queries, and exactly zero immediate limit flags. Query failure, no membership or nonzero flags retains the first refusal. No routing, public DTO, evidence or source-sealing code changes.

The author's Rust 1.99 formatting, host/Windows all-target Clippy and focused macOS GUI-death process regression pass. Its initial sandbox loopback refusal was excluded. A fresh independent Windows lifetime review and actual Windows core/engine/export/packaging CI remain pending. Earlier macOS matrices do not count as Windows execution of the new helper. The final native Retry/export/screenshot continuation is still blocked by the locked Mac.

The fresh source review is complete at `/private/tmp/tauri-network-windows-lifetime-review.md`, with no actionable findings. It independently checks the pinned WinAPI ABI, flag replacement, saved error, pseudo-handle and fixture cleanup behavior, exact measured Job JSON and matching integrated source hashes. The coordinator also ran integrated Rust 1.99 formatting and host/Windows-target all-target Clippy successfully; the actual output is `/private/tmp/tauri-network-final-compiler-checks.log`. [Run 39490806296](https://github.com/raymondctc/mobile-capture/actions/runs/39490806296) is executing the actual core, engine and bundle gates at source revision `1bcc69b65a48851706b0b7e465a2d61eeffe63d0`. Windows runtime acceptance and final native UI continuation are still pending.
