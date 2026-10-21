# Network timeline preview verification

Issue [17](https://github.com/raymondctc/mobile-capture/issues/17) tracks Network entries disappearing during capture under a busy device log stream.

## Cause and change

The guardian previously kept the global latest 500 events before the UI applied its source filter. Later device logs could evict every network event while cumulative network coverage and raw evidence remained present.

A shared helper now allocates the same total 500 slots fairly across present sources, redistributes unused capacity, and retains each source's newest entries in chronological order. Both network and log-only previews use it. The UI and README describe the preview and its search scope. IPC, polling, source classification, lifecycle, capture admission, and evidence export are unchanged.

## Failing and passing regression

The test-only commit precedes the fix. After 599 normalized log events, the old preview contained 0 network events where 2 were expected. Stop, raw export, normalized export, and cumulative counts had already passed before that assertion.

The same test now retains both network entries in live and stopped previews. Its receipt-ready guard ensures logs are complete before comparing counts across snapshots.

Passed 7 core tests, all 16 capture tests, and 3 focused network tests. These include source fairness, unused capacity, chronological ties, marker retention, raw and normalized export completeness, partial exports, and the actual serialized Snapshot reply below the existing 4 MiB frame bound. Strict core Clippy, workspace formatting, web build, and diff checks passed.

The core and all test targets passed the Windows GNU compile check with the explicit Rustup 1.99 toolchain. Native Windows execution and the full crash-boundary matrix remain for ordinary CI.

## Native app verification

The old native fixture showed its Network request at 295 logs and No matching recent events at 509 logs with Network still selected. Its stopped preview had 500 logs and zero network entries, while the ZIP retained the synthetic network record.

The rebuilt native fixture showed both injected Network entries at 539 logs during Recording. Direct snapshot inspection at 710 logs still found both entries within exactly 500 preview events and phase recording. It stopped at its duration bound with 786 logs. Network selection continued to show both entries; searching GET showed one, and clearing search restored two.

The native ZIP contained all 788 normalized events, including both raw network records. Fake routing was restored before fixture shutdown. No real phone capture, routing change, or certificate installation was performed. The rebuilt user app reopened Ready to capture with Pixel 4a listed.

## Review and limits

Independent correctness and no-comments reviews found no actionable issues in the exact source tree integrated into this branch.

A runtime preview cached by an older guardian keeps its old selection until a guardian rebuilds it. The change does not reconstruct legacy offline caches. Complete raw evidence and exports are unaffected. The preview remains a shared 500-event window, rather than a separate 500 events for each filter.
