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

## Actual Cargo Job boundary and current acceptance

The zero-only fallback above did not pass the actual Windows suite. In run `39490806296`, twenty of twenty-two network tests still refused guardian launch before routing. The standalone Python measurement had not described the Rust test caller. Commits `54fb8cd` and `669a29d` therefore add a fixture-only diagnostic and an early focused test that runs both the Python lifetime probe and an actual guardian Start/Stop from inside the Rust test process.

[Run 39492545299](https://github.com/raymondctc/mobile-capture/actions/runs/39492545299) measured successful current-Job membership and limit queries, immediate flags `8192` (`KILL_ON_JOB_CLOSE`), first launch error 5, and no retry under the zero-only policy. Its nested lifetime probe confirmed that detached children outside the GUI-owned source Job survive GUI termination, while its contained source child dies. `/private/tmp/tauri-network-ci-39492545299-windows.log:793` is pre-fix evidence; this failed run is not post-fix acceptance.

The design was revised before the next policy edit. Commit `44c05cb0db6a43ed752cafb0d962990f91600447` still prefers breakaway, but permits the detached/process-group retry after error 5 only when successful membership and limit queries establish exactly zero flags or exactly `KILL_ON_JOB_CLOSE`. Any other bit, failed query or no membership preserves the original refusal. External launcher containment remains effective and may interrupt the guardian, including during GUI exit; reopening with the original device is then required for recovery. No host Job policy is changed. The query describes the immediate Job, not every ancestor.

A fresh independent review at `/private/tmp/tauri-network-cargo-job-final-review.md` found no blocking findings and records exact source hashes. Rust 1.99 formatting and host/Windows-target all-target Clippy pass in `/private/tmp/tauri-network-cargo-job-compiler-checks.log`. In [PR run 39493387691](https://github.com/raymondctc/mobile-capture/actions/runs/39493387691), the focused Windows Rust-context test passed on this source, including actual fixture guardian Start/Stop. Full Windows and macOS `npm run verify`, real-engine probes and bundle creation are still running at this checkpoint. A duplicate push run was cancelled after preserving the focused result; the PR run remains active.

The earlier macOS job `118854841789` in [run 39488430726](https://github.com/raymondctc/mobile-capture/actions/runs/39488430726) passed all 48 Rust tests, the real-engine probe and app bundle/upload at source `42783f1`. Its network suite contained 23 tests with no skips. This includes the bounded ownership handoff, but predates the Windows launch diagnostics and policy; it is not represented as exact-source acceptance of `44c05cb`.

The user unlocked the Mac after the blocked CUA observation. The coordinator resumed the same `b83e591` fixture app: Retry recovery reached Stopped, Export produced the recovered `tauri-network-OHTMUA` ZIP, and selecting its HTTPS response showed `1 ms · host clock`, the canonical raw reference and the UTF-8 body. A screenshot confirms the native view. The app was then closed through CUA for a build of the final reviewed source. Consecutive capture and log-only UI checks remain pending at this checkpoint.

## Final packaged macOS continuation

The recovered native ZIP passed the independent checker and offline replay. SHA256 `9e307d94887f29054a62ebc21158a45ca2ce6938b8207f24f8bceb4ee258822a` contains eleven files, 784 log events, four network events, two HAR/native flows and no pending or unavailable bodies. It remains partial because of the omitted raw-header flow and malformed fixture log. Result: `/private/tmp/tauri-network-native-recovered-archive.json`.

The final app build at source `44c05cb` succeeded; `/private/tmp/tauri-network-final-native-build.log` records the build and `/private/tmp/tauri-network-final-native-hashes.json` identifies all three packaged binaries. The first manual launch omitted the required `MOBILE_CAPTURE_POC_FIXTURE_RUNTIME_ROOT` and aborted before setup. CUA then opened a normal unconsented app, which was closed without capture. The corrected launch supplied that fixture-only variable, displayed the fixture banner and recovered the same stopped session. That launch mistake is excluded from fixture verification.

Through CUA, the final app exported the reopened session, started network session `tauri-network-FNX5zU`, captured controlled HTTP and privately trusted HTTPS, stopped with verified restoration, exported and started another network session successfully. The second network session also stopped with restoration. This closes the native consecutive-capture gate after the bounded handoff fix. The final app ZIP passed independent hashes, canonical identities, raw body comparisons and native replay. SHA256 `88a8995a3ce655296aa9e7999096fe127296df9d18c5eb685621f1ba844d6afd` has eleven files, 113 log events, four network events and two HAR/native flows. Fixture malformed logs keep the package partial; the collector reports no network gaps. Result: `/private/tmp/tauri-network-final-native-archive.json`.

The coordinator then disabled network consent and drove a log-only capture, marker, Stop and Export in the same app. `tauri-poc-eSxG4W` contains seven expected members, 63 logs, one marker, zero network events, `capture_network=false`, `network=not_selected` and a reaped child. Every manifest artifact and SHA256SUMS entry matches the actual ZIP bytes. Its SHA256 is `56653c738bb40d58fec96ff9469a1bb0fb28f87c87c0df069f79b7793285da98`; result: `/private/tmp/tauri-network-final-native-log-only.json`. CUA screenshots confirmed the network host-clock/body detail and log-only export views. Fake ADB settings and reverse maps ended empty, the device remained connected and no `kill-server` command occurred. The app was closed through CUA after completion.

These native gates are complete on macOS with a compiled fake Android device and a real installed proxy. They do not establish physical Android routing, manual certificate installation, pinned-app interception or native Windows UI behavior. Host trust and real device routing were unchanged.

The fresh GPT-6 Sol closure audit at `/private/tmp/tauri-network-native-trail-closure.md` independently read all three native ZIPs and matched all three packaged binary hashes. It closes the prior missing Windows policy row and stale native evidence flags. In run transcript `01a1580c-d248-7582-8318-0d2c9975cdca`, ordinals 6585–6624 contain recovery/export and the host-clock screenshot; 6705–6812 contain the final app and consecutive network captures; 6814–6856 contain log-only controls and export. Screenshots were observed in the tool transcript, with no separate image file claimed. Full CI remained pending at that audit checkpoint. The previously disclosed historical timestamp edit is not rewritten.

## Final platform acceptance

[PR run 39493387691](https://github.com/raymondctc/mobile-capture/actions/runs/39493387691) completed successfully for source head `44c05cb0db6a43ed752cafb0d962990f91600447`. Both jobs checked out PR merge `80c843dcfd484e96c4f1dfb5d16bacad9f6c1135`, merging that head into foundation `25da77a8761dd1a279dc978ef9ed50cd271f4db2`. A direct GitHub tree query in `/private/tmp/tauri-network-ci-39493387691-tested-tree.json` confirms that its product, workflow and Python evidence identities exactly match the reviewed source. Both jobs passed formatting, Clippy, the full verification suite, installed mitmdump 12.2.3 evidence probes, release packaging and artifact upload. macOS passed 48 Rust tests; Windows passed 42, plus its separate focused Rust-caller launch probe. Both full suites report zero failed or ignored tests. Platform-specific Unix process tests are compiled only on Unix, rather than counted as Windows execution. The network suites each passed 23 tests, including the crash-boundary matrix, actual GUI process death, consecutive handoff, failed restoration/retry, sealed export, source-lock refusal and fragmented control frames.

On each runner, the real-engine result confirms original native/HAR/JSONL IDs and raw-body parity, binary/gzip forwarding, private-client TLS trust, bounded admission, oversized unavailable bodies, graceful authenticated shutdown, source-writer faults and binary-header omission. All five actual Rust owner/guardian cases restored fake routing. The successful source reports complete network coverage; TLS, HTTP, flush and binary-header cases report partial network coverage. These are real Windows and macOS processes and real proxy execution against a compiled private fake Android database, not physical phone routing.

| Platform | Completed job | Uploaded artifact | ZIP bytes | Uploaded ZIP SHA256 |
| --- | --- | --- | ---: | --- |
| Windows | [118872068546](https://github.com/raymondctc/mobile-capture/actions/runs/39493387691/job/118872068546) | [NSIS bundle 12297209274](https://github.com/raymondctc/mobile-capture/actions/runs/39493387691/artifacts/12297209274) | 2,940,742 | `4b2bc055999e2719c678981e910b725de098968d28ab02af13d8c2351ee15c7d` |
| macOS | [118872068862](https://github.com/raymondctc/mobile-capture/actions/runs/39493387691/job/118872068862) | [App bundle 12299100416](https://github.com/raymondctc/mobile-capture/actions/runs/39493387691/artifacts/12299100416) | 5,491,478 | `4fed737230444dc7c5b128529ac323a640996f4a9c6c5c6c46d10b6c6c4bf719` |

The Windows log is `/private/tmp/tauri-network-ci-39493387691-windows.log`; the macOS log is `/private/tmp/tauri-network-ci-39493387691-macos.log`. Actual run/job metadata and artifact digests are preserved in `/private/tmp/tauri-network-ci-39493387691-{run,status,artifacts}.json`. No post-fix claim relies on an earlier failed run. The deliberately cancelled duplicate push run is not a successful acceptance run.

The final evidence-only commit changes `.audit` records. The tested `TauriPoC` Git tree is `80a1b4fd896fa999285c7f486a05c8b7b443992b`; workflow blob is `c91ffb94cbcdef6ee48ad5a54aa719ec78165fcb`; unchanged shipping Python evidence blob is `5a6ec56c82b6edfdd30bf69f8b3318c89792892d`. Delivery must preserve those identities. A new PR synchronization may start another matrix for the audit commit; that run is distinct from the completed source acceptance above. Native Windows UI, physical Android routing and manual device CA installation remain outside this task's verified fixture scope.

The fresh GPT-6 Luna final audit at `/private/tmp/tauri-network-ci-closure-review.md` independently checked both completed logs, run/job metadata, artifact digests and the actual PR merge tree. It accepts CI/evidence closure for the tested source, resolves the earlier missing-row, stale-native and pending-full-CI flags, and preserves the fixture and later-PR-sync limits above. All review agents were explicitly stopped before final delivery.
