# Tauri network capture plan

Issue: https://github.com/raymondctc/mobile-capture/issues/9

Base: PR #8, `25da77a8761dd1a279dc978ef9ed50cd271f4db2`. Work is isolated to the Tauri PoC and its CI. The shipping Swift app and release configuration stay outside this slice.

## Outcome

An authorized Android device can route whole-device HTTP(S) through an owned loopback mitmproxy using ADB reverse. The interface shows network events and recovery outcomes. Export preserves real native flows, HAR, body bytes, canonical timeline records, and truthful completeness. CA trust is manual. App scope, iPhone, host proxy changes, and production engine distribution are outside this PoC.

## Work units

1. Ground routing, evidence and history; compare two ownership designs; save one reconciled contract.
2. Build persistent fake-tool probes for routing intent, startup rollback, conflict preservation, disconnect, repeated stop, parent loss and stale recovery. The tools must exercise subprocess and disk boundaries without reaching phones.
3. Implement the real engine and the selected process/routing owner. Restore and verify routing while the proxy is alive; keep recovery possible on failure.
4. Integrate network normalization, evidence, API and native controls. Preserve the log-only path.
5. Exercise real mitmproxy with controlled loopback HTTP(S), drive the packaged application, obtain independent reviews and exact-head macOS/Windows CI, then publish a stacked PR.

## Throughput checkpoint

The bottleneck is the recovery contract, not UI scaffolding. Two independent source readers have completed nonoverlapping routing and evidence investigations. One explainer merges them; two design candidates and a separate judge select one shape. A single strongest code owner implements the shared lifecycle in an isolated checkout. The coordinator owns integration, external delivery, and independent artifact verification. Reviews start only against a stable implementation. Reusable process probes carry more value than parallel writers that would disagree about session state.

Execution deviated from the last review timing sentence. A provisional independent review began while the backend writer investigated crash-test refusals. It read source hashes and built only private snapshots, then rechecked changed files. Its findings were not a final stable-source verdict. A fresh independent reviewer now checks the final delta and records its complete source inventory before integration. No reviewer ran suites or wrote code in the active author's checkout.

## Verification boundary

No real device routing, host proxy, or certificate trust changes during this task. Fake ADB uses a persistent private settings database. The real proxy binds loopback and serves controlled traffic. Fixture UI and evidence must identify simulation explicitly. Windows CI evidence is distinct from physical Windows/Android acceptance.

## Baseline and documentation

`npm run verify` passed all 21 existing macOS tests and six Rust/Python canonical event comparisons before edits. Output: `/private/tmp/tauri-network-baseline.log`.

Current mitmdump is 12.2.3, Python 3.14.4, OpenSSL 3.5.5 on macOS arm64. The existing repository engine lock is macOS arm64 only. The design must explicitly choose an installed PoC prerequisite or a separately reviewed portable payload; it cannot claim Windows engine packaging from that lock.

Context7 was resolved once and queried twice outside the sandbox, exhausting the three-call budget for this question. Official stable documentation confirms regular mode, loopback listener options, private `confdir`, and streaming native-flow output. `running()` means the proxy is ready; `done()` runs after the event loop stops; addon `ctx.master.shutdown()` requests graceful shutdown on both platforms. A private control mechanism can use that API without console signals.

Sources: https://docs.mitmproxy.org/stable/concepts/options ; https://docs.mitmproxy.org/stable/api/events.html ; https://docs.mitmproxy.org/stable/addons/examples .

The official [12.2.3 package metadata](https://pypi.org/project/mitmproxy/12.2.3/) confirms Python 3.12 or newer and lists Windows and macOS. This supports the existing CI Python 3.12 selection. It does not replace an actual engine run on either runner.

## Rationale coverage

Source history, local design/proof documents, tests, and directly linked GitHub records are the relevant record. Source-control grounding is in the routing investigation; an issue investigator checks PR #8 and issue #9. Google Drive/Pages are available but have no connection to this repository or its recorded design: no external document is linked, and the source records contain the physical Android finding, so unrelated personal-document searches are irrelevant. No team-chat, infrastructure-observability, error-tracking, or analytics connector is available for this project. Those records cannot be searched here. The unified grounding separates observed records from inference.
