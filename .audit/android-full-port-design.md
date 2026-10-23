# Android desktop integration design

## Synthesis decision

Candidate B is the base. The independent GPT-6 Luna judge and root selected its separation of capture authority and evidence reading. Candidate A contributes explicit operation states, cancellation at the publication boundary, and single-writer responsibilities. Both proposals preserve the guardian. Public Workspace and CaptureAuthority wrappers were rejected because the existing owner already protects capture operations. The private desktop coordinator earns its role by admitting mutually exclusive setup, save, capture, and quit actions.

The judge scored written proposals and grounding. Its requested checkout path had a typo, which it reported. Root inspected the actual source at 941074c9 and both full proposals before accepting the choice. The design decision is verified as a source-grounded choice, not a runtime parity claim.

## Shape

Keep existing CaptureOwner and guardian. A separate concrete EvidenceLibrary owns only derived index/cache writes and safe reads. The desktop root arbitrates operations and owns adapter/API/UI. No trait facade or new daemon.

Collection unit owns existing core model/controller/owner/worker/network integration and a new android module. Preserve CaptureSelection::Logs and LogsAndNetwork plus add Network. Add LogScope tagged by mode, with App { application_id } and WholeDevice. StartRequest retains serial, whole_device_consent, selection, limits; add log_scope, network_policy, and bounded CaptureContext. Old serialized requests decode with safe defaults, while every Rust initializer migrates in the same unit. Invalid app scope must fail before routing, never silently broaden. Whole-device consent is per recording. NetworkConsent remains explicit. Expose applications(serial).

Log limits allow up to200000000 bytes,250000records,600000ms, with production defaults; existing small tests specify small values. Network policy is independent and explicitly persisted. Default provisionally10000 flows,32MiB/body,128KiBheaders,1GiB aggregate,1hour, adjustable within validated hard budgets. Treat those as explicit user admission limits, not preview limits. Full admitted body bytes remain unchanged. Network does not stop when logs hit limits.

A versioned public descriptor must persist device, started_at, log selection/scope/limits, network policy, fixture and bounded CaptureContext { title, notes, steps }. It is not recovery or sealing authority. Collection owns descriptor writes, optional logging receipts and controller open_saved/recover/export integration. Existing legacy sessions stay readable and never mint proof.

Evidence unit owns new library/query/index modules and tests. It can inspect current and old log/network session.json formats; root passes authorized session folder. It never changes routing/collector/CA/evidence, except disposable cache in separately designated cache path. Use safe existing evidence file admission. Query incremental bounded ingestion; search complete details and decoded body text; generation-bound cursors and total/matching/indexed/backlog. Full event detail and bounded body reads independent of page. Source fields and event IDs match current normalizer.

Delivery unit owns only Tauri packaging scripts/locks/resources metadata, tools.rs, doctor module/CLI and tests, CI config and Tauri product metadata; root owns actual adapter wiring. Resource resolution packaged-first, damaged bundle fails closed, explicit dev overrides labeled. Preserve dev.mobilecapture.tauri-poc identifier and current data/CA paths, visible name Mobile Capture Android. macOS ARM64 and Windows x64 are initial complete targets. No Swift source or existing release-pipeline changes.

Save and setup are subsequent units after core/libraries. Root owns native dialogs/reveal/settings/lifetime, asynchronous operation state, setup isolation and final integration. Export and recovery still need existing verified source/restoration proofs. Read-only library flags never authorize export.

## Accepted implementation reconciliation

The collection writer puts new version-1 public facts in capture.json and preserves existing session.json operational formats. Root accepted this to avoid mutating the guardian's existing Header schema. SessionView adds descriptor_version, selection, log_scope, network_policy, context, and log_attribution. Legacy serialized requests decode with whole-device scope, which still requires explicit consent. New UI defaults to app scope.

NetworkPolicy fields are max_flows, max_body_bytes, max_header_bytes, max_total_bytes, and duration_ms. The total budget is between 1 MiB and 1 GiB. Maximum body size is 32 MiB. Duration is admission duration. The addon continues forwarding after admission closes until explicit Stop restores routing. Neither source limit implicitly stops the other selected source.

The library is concrete EvidenceLibrary in capture-core/src/library. Its disposable bundled-SQLite index stays outside evidence. Canonical Unicode folding and bounded gzip/deflate decoding support complete search. Original body chunks keep raw_content basis. Decoded chunks have content basis and refuse inflation above 32 MiB instead of claiming a prefix is complete. Its exported types and signatures live in library/query.rs and library/mod.rs. Root's typed IPC schemas are in src/product.ts.

Delivery stages verified workers, addon, ADB, and the standalone engine in one android-tools resource payload. The payload digest is compiled into the app and Doctor. Packaging signs nested resources before hashing and signs the outer app after Tauri copies resources. Tauri materializes upstream framework symlinks, so the packaging script restores the exact verified tree before outer signing. The installed clean-PATH Doctor check caught and verified this correction.

## Verification gates

The phase plan and checkbox record live beside this design. Each implementation unit requires behavior tests and an independent review. The whole product requires a packaged native fixture flow and native Windows CI with standalone-payload smoke. Publisher signing, notarization, clean-host physical-phone trust, and hardware acceptance remain distinct observations. Source feasibility cannot satisfy those gates.
