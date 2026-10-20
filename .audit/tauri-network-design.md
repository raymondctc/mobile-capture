# Tauri network design. The guardian owns the whole network session

## Problem

An Android user can select whole-device logs with optional HTTP(S) capture, close or lose the GUI, reconnect, and recover the connection before exporting evidence. For a network session, one detached guardian owns routing, the proxy, the log collector, markers, source completion, and export eligibility. The GUI is its client. Log-only capture retains its existing GUI-owned lifetime and verified-receipt behavior.

The grounding establishes a required ordering and a physical Android cache failure. It does not establish that a proxy can survive its own guardian's hard death. Issue and comment text is an agent-produced working contract, not independent human testimony. Sources are `/private/tmp/tauri-network-grounding.md`, `.audit/tauri-network-rationale.md`, `AndroidCaptureRouting.swift`, and schema 1.

## Usage, written before the types

The start screen retains log consent and adds optional network consent, a configured mitmdump prerequisite, and a manual CA-trust explanation. Network capture applies to the selected Android device. Trust installation remains manual. The UI offers the public certificate separately and labels the nonce-bound browser check as synthetic.

```rust
// The Tauri adapter holds this facade. It never owns network children.
let capture = CaptureController::open(paths, tools)?;
let view = capture.start(StartRequest {
    device: selected, consent: whole_device_consent,
    selection: CaptureSelection::LogsAndNetwork(network_consent), limits,
})?;
render(view); // Recording, or a typed refusal with the recovery action.

// A normal session uses the same outcomes in either capture mode.
capture.marker(MarkerRequest::new(action_id, "Reproduced login failure"))?;
render(capture.snapshot()?);
match capture.stop()? {
    StopOutcome::Ready(view) => enable_export(view),
    StopOutcome::RecoveryRequired(view) => show_restore_retry(view),
    StopOutcome::Unverified(view) => show_shutdown_blocked(view),
}

// A restarted GUI discovers the global pending session across output roots.
let view = capture.snapshot()?; // Reads or attaches. Does not change routing.
render(capture.recover()?); // Reuses a live guardian or starts a recovery guardian.
let archive = capture.export()?; // Refuses until restoration and writer quiescence.
```

GUI close requests Stop and displays a retryable error while the window exists. GUI death closes its client lease and causes the guardian to perform the same Stop. Neither action drops a proxy handle. A failed restoration keeps the listener, journal, and global lease alive. A restarted GUI shows **Restore connection and stop**, the affected device, the failed acknowledgment or conflict, and **Retry**. Export remains disabled.

## Shape

```rust
pub enum CaptureSelection { Logs, LogsAndNetwork(NetworkConsent) }
pub struct StartRequest { device: ReadyAndroid, consent: LogConsent,
    selection: CaptureSelection, limits: CaptureLimits }
pub struct CaptureController { backend: Backend } // Backend is private.
impl CaptureController {
    pub fn open(paths: CapturePaths, tools: ToolSelection) -> Result<Self>;
    pub fn start(&mut self, request: StartRequest) -> Result<Snapshot>;
    pub fn snapshot(&mut self) -> Result<Snapshot>;
    pub fn marker(&mut self, request: MarkerRequest) -> Result<Snapshot>;
    pub fn stop(&mut self) -> Result<StopOutcome>;
    pub fn recover(&mut self) -> Result<StopOutcome>;
    pub fn export(&mut self) -> Result<Archive>;
}
pub enum StopOutcome { Ready(SessionView), RecoveryRequired(SessionView),
    Unverified(SessionView) }
pub enum NetworkView { NotSelected, Preparing, Recording(CoverageView),
    RecoveryRequired(RecoveryProblem), Stopped(NetworkCompletion),
    Interrupted(NetworkCompletion) }
// Public DTOs describe outcomes. No PIDs, socket frames, journal flags, or stages.

// The following items are private to the network module.
struct Guardian { lease: RoutingLease, session: NetworkSession, control: Control }
enum NetworkSession {
    Preparing(PreparingSession), Recording(LiveSession),
    Restoring(LiveSession), RecoveryRequired(LiveSession, RecoveryProblem),
    Draining(RestoredSession), Ready(SealedSession), Unverified(UnverifiedSession),
}
struct LiveSession { route: RouteJournal, collectors: OwnedCollectors,
    markers: MarkerWriter, started_at: HostTime, deadline: Instant }
struct OwnedCollectors { logs: LogCollector, proxy: EngineChild }
struct RestoredSession { route: VerifiedRestoration, collectors: OwnedCollectors,
    markers: MarkerWriter, coverage: CoverageFacts }
struct SealedSession { restoration: VerifiedRestoration, evidence: SealedEvidence }
struct SealedEvidence { sources: QuiescentSources, completion: CompletionFacts }
enum SettingValue { Absent, Present(String) }
struct ProxySettings { http_proxy: SettingValue, host: SettingValue,
    port: SettingValue, exclusions: SettingValue, pac_url: SettingValue }
struct RouteJournal { device: DeviceId, adb: AbsoluteExecutable,
    before: ProxySettings, applied: ProxySettings, endpoint: ReverseEndpoint,
    operations: Vec<RouteOperation>, restore_attempt: RestoreAttempt }
struct RouteOperation { intent: Mutation, allowed_after: ProxySettings,
    result: Option<VerifiedObservation> }
enum Mutation { AddReverse(ReverseMapping), Put(ProxyKey, SettingValue),
    ClearEmpty, ClearDisabled, RemoveReverse(ReverseMapping) }
impl Guardian {
    fn start(&mut self, request: ValidatedStart) -> Result<Snapshot>;
    fn stop(&mut self, cause: StopCause) -> StopOutcome;
    fn recover(&mut self) -> StopOutcome;
    fn export(&mut self) -> Result<Archive>;
}
impl EngineChild {
    fn launch(config: EngineConfig, mode: EngineMode) -> Result<Self>;
    fn stop_and_verify(self) -> Result<EngineCompletion>;
}
enum EngineMode { Capture(NetworkPolicy), RecoveryForwarding }
fn routing_step(journal: &RouteJournal, observed: &AndroidState,
    goal: RoutingGoal) -> Result<NextRoutingAction>; // Pure reconciliation.
fn export(sealed: &SealedSession) -> Result<Archive>; // Cannot accept LiveSession.
```

The controller selects one lifetime policy from `CaptureSelection`. The existing owner becomes private `LogSession`; its existing calls and receipt semantics remain. Extract its child lease into `LogCollector` so the guardian can own the same bounded worker without nesting another session owner. There is one public start, stop, marker, and export path for each selected session. The GUI cannot import the guardian's routing or collector internals.

Each child writes its own raw source files and receipt. The guardian alone writes session state, routing intent, and markers. The exporter derives counts and completeness from immutable source facts. It does not maintain a second session-status database. Per `separate-before-serializing-shared-state`, source writers do not patch fields in a shared JSON document. Per `model-the-domain`, only verified restoration can construct `RestoredSession`, and only quiescent sources can construct `SealedEvidence`.

## Process and control ownership

Bundle `capture-guardian` beside `collector-worker`. On macOS, detach the guardian's process session and keep its standard streams independent from GUI lifetime. On Windows, launch outside the GUI-owned collector Job. If inherited Job restrictions prevent independent launch, fail before any routing mutation. The guardian may create its own kill-on-close child Job, which contains its proxy and log worker, never itself. Never run `adb kill-server`.

Use one authenticated, bounded JSON control protocol on a random loopback TCP port on both platforms. Bind only 127.0.0.1. Publish the port and a 256-bit random capability only in the private session directory. This is not HTTP and has no proxy shutdown URL. Keep the protocol fixed to the six outcomes above, plus the startup attachment handshake. Length-limit frames and validate them at this boundary. Generate a random session capability in the private runtime directory. The GUI client lease has a distinct persistent connection; its EOF requests Stop. Closing an individual snapshot request does not. No HTTP control endpoint exists, including inside the proxy.

The proxy addon watches a private nonce-bound stop file and an inherited guardian lease pipe. A stop-file request or guardian EOF requests `ctx.master.shutdown()` on the engine event loop. The stop-file mechanism already has a real 12.2.3 HTTP/HTTPS proof in `/private/tmp/tauri-network-engine-probe.py`; pipe EOF still needs its process test. `running()` publishes nonce-bound readiness; `done()` closes native/HAR writers and writes a completion receipt. The guardian waits for the owned child handle and receipt. After restoration it can escalate a hung owned child to termination, then record a forced partial outcome. It never signals a PID from durable state.

Take macOS `/tmp/capture-companion-<uid>/owner.lock` with the same `flock` and ownership checks as production. Reuse its inode, secure historical mode 644, and write the absolute session path in place. Do not rename or unlink it. Maintain native-compatible `session.json` phase and `routing_restored` fields so production refuses a pending Tauri session. Windows uses one current-user `%LOCALAPPDATA%/MobileCapture/owner.lock` with `LockFileEx`, shared by every PoC output root. A private control descriptor belongs to the referenced session, not a second global locator.

A failed ping establishes only an unreachable guardian. Recovery must acquire the same OS lease before declaring the guardian absent. Startup checks the prior session path before replacing lock contents. Corrupt or unrestored prior state blocks a new session. Recovery uses the saved device and executable, even if the GUI selects another device or output root.

## Routing and every crash boundary

Persist each intent by syncing a private temporary file, atomically replacing the journal, and syncing its parent where supported. Windows uses file flush plus a write-through replacement. Persist the session header before publishing its path in the held global lock. No reverse or settings command can precede its intent. Per `make-operations-idempotent`, observations determine the next action after any retry.

| Crash boundary | Reconciliation and invariant |
| --- | --- |
| Before the session header or lock-path publication | No device mutation exists. An unpublished private directory is inert. |
| After publication, before proxy readiness | The journal says no mutation. Stop children, prove quiescence, and seal startup failure. |
| After readiness, before reverse intent | Android still has its original state. A failed start can stop the listener. |
| After reverse intent, before or after `--no-rebind` | Read mappings. Absent means no removal needed. The exact journaled source and target may be ours. Any other target is a preserved conflict. |
| After proxy intent, before or after each key write | Read all five fields. Each must equal its original value, intended applied value, or a durably recorded transition. Preserve and report any other value. Reconcile partial application. |
| After the last write, before applied verification | Verify the entire applied snapshot. A command exit code alone never establishes routing. Failure enters Stop while the proxy lives. |
| After a restore-attempt record, before each clear command | Re-read the whole snapshot and refuse unknown values before changing another field. The proxy and reverse remain. |
| After empty intent or command, before empty acknowledgment | Only the journaled empty transition is admissible. Verify it, then journal a fresh `:0` intent. |
| After `:0` intent or command, before normalized acknowledgment | Wait for the complete normalized cleared vector. On a timed-out prior `:0`, retry through the journaled empty transition before another `:0`. |
| After clear acknowledgment, before or after each exact restore | Restore absent with delete and present with its exact string, including empty strings. Re-read the full snapshot before each mutation and verify exact equality at the end. |
| After exact restore, before reverse removal intent or command | Keep the listener alive. Remove only the exact owned mapping. Refuse an external mapping. Verify endpoint absence before recording restoration. |
| After reverse absence, before restoration commit | Re-read settings and mappings. Repeating verification converges without deleting another mapping. |
| After restoration commit, before or during child shutdown | Android no longer needs this listener. Finish child stop, source verification, and sealing. Never restart capture into these evidence files. |
| After sealing, during ZIP staging or publication | Sealed evidence is immutable. Discard an unpublished stage and repeat export. Publish atomically after checksum verification. |

Use the existing reverse key order and `--no-rebind` ownership rule. For an originally disabled proxy, reproduce the five-field normalized clear acknowledgment before restoring exact originals. An originally active proxy gets conflict checks and exact restoration without the disable sequence. An ADB disconnect or external-key conflict enters `RecoveryRequired`; Stop never closes children in that state. Admission can end while the listener continues forwarding indefinitely.

## Guardian hard death

Hard guardian death can interrupt the listener and evidence. State this explicitly in the UI and package. Each original collector holds a session-local source-writer OS lease for its entire writing lifetime. Guardian-owned pipe EOF stops the log worker and requests engine shutdown; Windows child containment can force termination. A later guardian acquires the global lease, waits for source leases to establish quiescence, and seals original evidence as interrupted. It does not treat source receipt PIDs as authority.

Before restoration, that recovery guardian starts a forwarding-only proxy on the original loopback port with the original private CA. This proxy writes no original network evidence. Its readiness restores a listener for retries, not continuous capture. If the port remains occupied, recovery waits for owned-writer quiescence and retries binding; an unexplained remaining listener is a named recovery conflict. It never kills that process. On restoration failure, the new guardian retains its forwarding listener and journal. On success, it removes the owned reverse, gracefully stops the recovery proxy, then permits a partial package after all original writer leases are exclusive. Device disconnection remains retryable. A source that cannot be proved quiescent blocks export, even after routing is restored.

## Real engine and bounded original evidence

Use an explicitly configured installed mitmdump for this PoC. Preflight the supported pinned version, absolute executable, private CA directory, addon resource, public certificate, and actual readiness before routing. Record the engine version and policy in context. Windows README and UI state the installed-engine prerequisite. CI installs and executes that real version. Production redistribution, licenses for a bundled payload, and automatic trust are outside this slice.

Do not run the production addon unchanged or enable native `-w`. Bundle a PoC addon that reuses its setup probe and pure header/time helpers. Keep production behavior unchanged. The PoC addon owns a `FlowWriter`, streaming body callbacks, and a finite admission table. Initial policy admits at most 32 HTTP flows in 120 seconds, with at most 256 KiB per original request or response body and 16 KiB retained header metadata per message. The policy bounds retained bodies to 16 MiB before serialization. Enforce a 96 MiB aggregate network-artifact budget as an additional guard.

Reserve a slot at request headers. Stream every body onward unchanged while copying admitted bytes into the bounded slot. If a body exceeds its cap, discard its retained bytes and mark it unavailable in all outputs. Never present its prefix as a complete body. Unadmitted flows stream onward with no retained body or growing per-flow table. Keep saturating omission counters and a bounded sample of reasons. Close admission at duration, flow, or storage limits; forwarding remains available during routing recovery.

At terminal response or error, write one native flow copy using retained original `raw_content` bytes, one HAR entry, and the request/response event records. Preserve original content-encoding headers and declare the raw-byte basis in body metadata. Native, HAR base64 fields, and event body bytes must decode identically. Missing streamed bytes are `None` or explicitly unavailable, never empty. Use private append-only HAR-entry spooling and assemble HAR on `done()`. The admission table contains at most 32 slots and releases completed bodies. No whole-session HAR/body dictionary grows with traffic. Validate every serialized record against the aggregate budget before writing; reserve final incomplete/receipt space. Cross-file write failure marks partial and closes admission.

`done()` persists pending admitted flows, unavailable bodies, omissions, and error summaries in `network/incomplete.json`; no per-unadmitted-flow list grows. The guardian derives source status from these facts and process completion. Decoded request or response is required for `observed`. TLS-only remains `tls_failed`; pending flows, omissions, unavailable bodies, any forced stop, absent artifact, or guardian interruption make the package partial.

Extend Rust normalization with schema-1 network IDs derived from raw source, relative path, one-based line number, and raw record bytes. Public references become `network/events.jsonl#L<n>`. Preserve addon IDs and native references in detail. Preserve original and received timestamps, host uncertainty, and source ordering. A snapshot returns bounded previews and explicit preview lengths; canonical evidence retains admitted full bytes.

Export streams no-follow regular files into a private stage, hashes their original bytes, validates all timeline references, and streams ZIP output. The exact raw allowlist remains logs, markers, and the four documented network files. Runtime tokens, control descriptors, source locks, journal, private CA, readiness, and private spool never enter the ZIP. Replace the Rust whole-file 34 MiB assumption for packaging rather than silently applying it to network bodies. Stop restores and verifies settings, removes the owned reverse, stops and verifies both collectors, then seals and enables export.

## Module map and implementation boundary

| File or module | Knowledge it owns |
| --- | --- |
| `capture-core/src/owner.rs` | Public controller and unchanged private log-only session; domain outcomes. |
| `capture-core/src/network/` | Private guardian state machine, route journal/reconciliation, client outcomes, sealing. Split cohesive route, engine and control knowledge without exposing stages. |
| `capture-core/src/network/control.rs` | Fixed authenticated loopback framing, attachment, and process bootstrap. |
| `capture-core/src/process.rs` and `worker.rs` | Reusable owned log collector, live handles, platform containment, source-writer leases. |
| `capture-core/src/bin/capture-guardian.rs` | Thin guardian executable entry. |
| `TauriPoC/resources/network_addon.py` | Bounded proxy evidence, private shutdown pipe, existing synthetic setup probe. |
| `timeline.rs`, `evidence.rs`, `files.rs` | Canonical source normalization and streaming sealed export with existing file guards. |
| Tauri adapter, `api.ts`, `main.tsx`, packaging script | Domain validation, optional network controls, recovery rendering, two Rust binaries and addon. |

Per `boundary-discipline`, command output, IPC, journals, and addon facts parse once into domain types. Tauri adds consent and view adaptation, rather than a pass-through transport API. Route knowledge remains private in one module. A normal action reaches controller, guardian, then its owned resource without caller-visible stage choreography.

## Proof required for this slice

- Persistent fake ADB preserves absent versus empty values and records every command across subprocess deaths. For each table boundary, crash before and after the journal write, command, and verification. Restart and assert exact restoration, external-value preservation, same-to-same reverse ownership, and no shared-server termination.
- Process tests close the GUI lease, kill the GUI, kill the guardian, hang or kill the engine, disconnect ADB, lose a startup response, retry Stop, and recover from a different output root. A listener probe must succeed throughout every live-guardian failed restoration. Hard guardian death must report an interruption and acquire source leases before export.
- Test production-compatible macOS lock contention without changing its inode, historical permission tightening, and Windows cross-output-root exclusion. Exercise detached Windows launch and its actual Job lifetime in CI.
- Real installed mitmdump loopback tests send binary and content-encoded HTTP and HTTPS with a private client trust store. Use the existing nonce-bound `mitm.it` response with lazy connections and upstream certificate lookup disabled to avoid an external upstream. Replay native flows and compare original bytes against HAR and JSONL. Cross admission caps, force missing bodies, interrupt writes, and verify bounded retained state, partial outcomes, canonical IDs, checksums, and runtime-file exclusion.
- Drive a built native GUI for both modes. Observe filter/detail previews, whole-device consent, prerequisite failure, retry recovery, disabled export, and final package. Label fake-device evidence as fixture. macOS and Windows CI prove portable behavior; no physical Android or host-trust acceptance is claimed.

## Tradeoffs and alternatives

We accept moving network-session log ownership and adding one narrow private control protocol in exchange for one authority over Stop, routing recovery, and export. The public interface hides substantially more behavior than it exposes. Keeping the log-only lifetime avoids changing the approved foundation for users who do not select network capture.

A narrow routing guardian with GUI-owned log collection is a structurally different alternative. It hides Android mutation but leaves the GUI coordinating log receipts, marker closure, guardian restoration, source interruption, and export. It loses on interface depth and creates two session completion authorities. A guardian retaining routing but delegating all evidence to a second independent collector owner adds another recovery relationship without reducing the Android risks. Neither alternative is required for this PoC.

The bounded streaming addon is the largest additional implementation cost. We accept selective, explicit partial evidence in exchange for bounded memory and disk while keeping admitted original bytes. A streaming exporter alone would leave the current addon's unbounded retention and native writer intact. That does not satisfy a bounded PoC.

## Synthesis decision and reconciliation

The coordinator and independent GPT Luna judge selected candidate A as the base. The judge scored A 10/10 and B 8/10; the coordinator agreed that B adds a second completion boundary and leaves native disk admission unbounded. Grafts from B make the installed engine prerequisite explicit in UI/CI and retain a direct cross-output-root recovery test. No GUI-owned collection split was grafted.

The coordinator accepts a single authenticated loopback JSON protocol in place of separate Unix-socket and Windows-named-pipe transports. A private random capability, loopback binding, bounded frames and deadlines retain the control boundary while allowing one portable implementation. This does not introduce an HTTP or proxy shutdown endpoint. Existing `CaptureOwner` naming may be retained where renaming buys no invariant. Outcome operations stay singular; internal route stages stay private. The selected module map permits cohesive private route/engine/control files rather than one oversized network module.

The engine-control prototype already proved real HTTP, synthetic HTTPS and graceful addon-requested shutdown. Source-writer leases, bounded original body/native parity, recovery and actual Windows lifetime still require executable proof. No further deviation has been accepted. If private shutdown, original-byte streaming, source-writer leases, or independent Windows lifetime cannot be demonstrated, revise this contract before implementation rather than weakening the outcome silently.

The main remaining risk is whether the installed Windows engine and streamed flow copies honor the promised native/HAR parity. Resolve it with the first real-engine probe. The first implementation unit is the persistent fake-tool guardian process and crash-boundary recovery tests, followed immediately by the bounded real-engine loopback probe before UI work.

## Implementation reconciliation

The sketches above are the preimplementation contract. The implemented public facade is `CaptureOwner` in `controller.rs`; `owner.rs` contains private `LogSession` and reusable `LogCollector`. `StartRequest` accepts a device serial and explicit booleans, validated at the boundary. The public `CaptureSelection`, `NetworkView` and `StopOutcome` preserve the selected outcome model. No public controller exposes journal stages, PIDs or IPC frames.

The private guardian uses a persisted production-compatible `Header` plus owned `Option` fields rather than the sketched large `NetworkSession` enum. This is an accepted simplification. The critical completion boundary is still encoded in types. `SealedEvidence` contains `VerifiedRestoration` and both exclusive source-writer leases through `QuiescentSources`; the network exporter requires a reference to that capability. `Header.phase` remains a string to interoperate with the shipping macOS pending-session guard. A persisted terminal phase alone cannot grant export.

Routing reconciliation lives in private `Route` methods in `network/route.rs`, rather than the sketched standalone pure `routing_step`. The methods combine durable intent, full state observation and one next command. The immutable 144-boundary process matrix verifies the selected idempotence and conflict-preservation contract. Engine lifecycle is in `network/engine.rs`; authenticated control and the GUI ownership lease are in `network/control.rs`. Those cohesive files replace the draft module names without adding another completion owner.

The addon streams raw bytes, preserves original flow IDs in native copies, explicitly omits unsupported metadata and holds its writer lease until every initialized writer closes. The installed official macOS binary's `12.2.3 binary` version spelling is accepted. Failure, omission and absent graceful completion remain partial. The independently executed real-engine proof, including native/HAR/JSONL parity and close/fault cases, is recorded in the verification document. It does not establish physical Android trust behavior.

The native UI exposed one additional lifetime edge after recovery Export. Replacing a fixed 50 ms delay with a bounded observation of the actual owner lease is the selected correction. On refusal, the previous session remains visible and exportable. The next guardian still acquires the global lease itself before publishing or mutating routing. This handoff has its own focused proof and independent review gate.

Windows still requests a detached guardian outside the inherited GUI Job and refuses before mutation if launch restrictions prevent independence. Cross compilation cannot prove that lifetime. Actual Windows execution and packaging remain a required CI gate; no inherited-Job fallback is accepted without measured evidence and an explicit contract revision.

## Measured Windows launch revision

Run `39489663292` supplies the previously missing facts. The controller and fixture GUI belong to a launcher Job whose immediate limit flags are zero, with neither breakaway permission nor kill-on-close. The requested breakaway launch fails with Win32 error 5. Plain and detached/process-group children stay outside the separate GUI-owned source Job and survive handle-based GUI termination. The source child belongs to that GUI-owned kill-on-close Job and dies. All owned fixture handles are cleaned up. Exact JSON is in `/private/tmp/tauri-network-ci-39489663292-windows.log:141`. The immediate Job query does not disclose every ancestor policy.

The selected correction keeps the preferred breakaway launch. Only Win32 access-denied from that launch may retry without breakaway, and only when checked Job APIs report membership in an immediate launcher Job with zero limit flags. A failed query, no Job membership, or any immediate limit flags preserves refusal before routing. The retry retains detached/process-group flags. The guardian must remain outside every app-owned source Job; the GUI never assigns itself or the guardian to those Jobs.

This revises the earlier requirement to escape every inherited Job. The guaranteed lifetime is independent of the GUI process and its app-owned worker containment. External launcher/host containment can terminate the guardian; that is the existing hard-guardian-death recovery and partial-evidence boundary. The app will not change a launcher Job policy or bypass its containment. Full actual Windows guardian, GUI-death, restore/retry, engine and export tests remain required after this correction. No broader inherited-Job fallback is accepted from the probe alone.

The zero-only policy did not cover the actual Cargo caller. Run `39492545299` measures that boundary directly. Rust reports checked membership and query success, flags `8192` (`KILL_ON_JOB_CLOSE`), access-denied 5 and no retry. The same owned-handle experiment, launched inside the Rust test process, reports that plain and detached children survive GUI termination outside the GUI-owned source Job while its source child dies. `/private/tmp/tauri-network-ci-39492545299-windows.log:793` preserves both experiment layers and the Rust JSON. The shell's earlier zero flags were not proof of the Cargo caller policy.

Before the next policy edit, the accepted predicate is refined to allow only zero flags or exactly `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE` after the existing error, membership and query checks. Any other bit remains a refusal. Kill-on-close refers to closure of the external launcher's last Job handle, not termination of the fixture GUI; that distinction is now measured in the actual caller context. The external-host termination boundary remains explicit. Actual Rust guardian survival, the full Windows crash matrix, real engine and export still require the post-fix run.
