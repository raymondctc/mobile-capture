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

## Engine-control prototype

Before implementation, `/private/tmp/tauri-network-engine-probe.py` ran installed mitmdump 12.2.3 with the existing nonce-bound setup response and a private addon watcher. It served one HTTP request and one HTTPS CONNECT request on loopback. The client trusted only the generated public CA in its own `SSLContext`. `connection_strategy=lazy` and `upstream_cert=false` kept the synthetic responses inside the proxy.

Writing a private stop request caused the addon to call `ctx.master.shutdown()`. Its `done()` hook ran and the engine exited 0. The run produced four raw request/response events, two HAR entries and a 5,660-byte native file. This proves graceful control and real TLS interception for this controlled path. It does not prove native replay parity, routing recovery, or full application support.

Result path: `/var/folders/9d/_w0dsyv95ys3r_6hd99pcxch0000gp/T/tauri-network-engine-proof-5qbgvpy1/result.json`. No phone or host proxy/trust settings changed.
