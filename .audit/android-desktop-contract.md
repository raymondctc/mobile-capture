# Desktop IPC contract

The root implements Rust commands. The UI writer owns TypeScript schemas and React. All invoke responses enter as unknown and pass Zod. These names and fields are accepted integration contracts. Changes require a message to the other owner before dependent code changes.

## Capture and evidence

- discover returns Device[].
- applications {serial} returns AndroidApplication[] from product.ts.
- start {request:StartRequest} returns Snapshot. StartRequest is defined in product.ts. Whole-device log consent and whole-device network consent are explicit for each recording. Defaults are network only and app scope for logs.
- snapshot returns lightweight CaptureOwner.status Snapshot. SessionView adds descriptor_version, selection, log_scope, network_policy, context, and nullable log_attribution. It has no full timeline rows. Library counts are the indexed counts.
- marker {text}, stop, recover return Snapshot. Marker text stays in the field if writing fails.
- sessions, open_session {directory}, timeline_query {query}, event_detail {id}, body_chunk {id,side,offset,limit,decoded}, detail_chunk {id,offset,limit} match product.ts. open_session changes only inspected evidence. It cannot replace active control.
- choose_folder returns string or null from the native dialog.
- reveal {target:'session'|'raw'|'archive'|'certificate',id:string|null} resolves a fixed admitted path on the host. Raw needs event ID. Archive with an ID resolves only the matching completed save operation, so its success banner is independent of the inspected session. It accepts no arbitrary command.

## Preferences and diagnostics

preferences returns {output_root:string,limits:LogLimits,network_policy:NetworkPolicy,adb_override:string|null,mitmdump_override:string|null}.

save_preferences {preferences} returns the same shape. Storage and tool changes require idle verified control. Overrides are explicit absolute executables. Bundled tools remain the default and damaged payloads never silently fall back. The UI may store non-authoritative display choices in validated localStorage.

doctor {sources:'logs'|'network'|'logsAndNetwork'} returns the installed core DoctorReport in camelCase. Fields are schemaVersion,command,scope,platform,checkedAt,hostTarget,hostPrerequisitesReady,sources,checks,deviceSetup,error. A check has id,status,message,source,path,version. Status is ready,needsAction,missing,incompatible. deviceSetup has status and manualActions. Host readiness does not verify the phone.

## Long operations

begin_save {directory:string|null} starts stop-and-save for current control when null, or verifies and reopens a selected stopped session before saving. It returns OperationView immediately. Retrying uses the same action. Export publication is atomic. An accepted cancellation preserves raw evidence and a previous ZIP.

copy_archive opens a native save dialog and copies the inspected verified archive to the selected location atomically. It returns string or null.

begin_certificate {serial} owns a separate network-only preparation capture, pushes only the public CA to Android Downloads, restores routing, then returns manual installation instructions. No phone-side trust automation.

begin_connection_check {serial} owns a separate network-only capture for up to45seconds. It uses the current CA and an exact fresh HTTPS nonce response. It leaves the inspected user session intact. The UI shows its URL, a copy action, an explicit open_check_on_phone command, and a phone-origin confirmation checkbox. A host request, certificate acknowledgment, or TLS error cannot pass. begin_check is an explicit capture action and requires the network consent disclosure in Setup.

confirm_check_origin {id,confirmed} records the user's phone-origin confirmation for that owned check. open_check_on_phone {id} opens only the current fixed HTTPS check URL on the selected phone through ADB. cancel_operation {id} cancels a save before publication or finishes a setup check and still restores routing. operation returns the latest OperationView. retry_setup_recovery {id} retries restoration for the failed setup owner.

OperationView is tagged by state.

- idle has no other fields.
- saving has id,stage:string,completed:number,total:number|null,cancel_allowed:boolean.
- preparing_certificate has id,stage:string,message:string.
- checking_connection has id,url,certificate_fingerprint,https_observed:boolean,phone_origin_confirmed:boolean,remaining_seconds:number.
- restoring has id,kind:OperationKind,message:string.
- finished has id and result:OperationResult.
- failed has id,kind:OperationKind,message,recovery_required:boolean,directory:string|null.
- cancelled has id,kind:OperationKind,message.

OperationKind is save,certificate,connection_check,readiness_report. OperationResult is tagged by kind.

- save has archive,directory.
- certificate has serial,certificate_fingerprint,certificate_path,device_path,routing_restored:boolean.
- connection_check has serial,directory,url,certificate_fingerprint,https_observed:boolean,phone_origin_confirmed:boolean,routing_restored:boolean,verified:boolean,checked_at,expires_at,message.
- readiness_report has path.

export_readiness_report uses the current public host/setup facts and a native destination dialog. It returns string or null. The report contains checks and public identities, no captured bodies/logs, private CA, or routing/control files.

open_updates opens the Android release page only while capture, setup, save, and recovery are idle. Updates are manual for this first port. It does not replace the Swift Sparkle feed or auto-install a package.

## Reading controls

Complete search uses the library, never only Snapshot.events. Source and search changes reset page cursors. Selected EventDetail persists outside the visible page. Pause and Clear View use through_sequence and after_sequence, while indexing and capture continue. Older/newer pages, newest/oldest ordering, line wrapping, follow preference, jump to latest, index backlog, and raw reveal are explicit. If a cursor invalidates, show the reason and reload a fresh page instead of silently dropping evidence.

## Lifecycle

Configuration and source changes cannot affect active recording. Window close keeps normal capture alive and shows how to reopen it. Quit waits for restoration and verified shutdown; unresolved recovery prevents exit. Setup checks never overwrite the inspected directory. Native hardware/Windows acceptance is separate from UI build success.
