import { invoke } from "@tauri-apps/api/core";
import { z } from "zod";
import {
  captureSelectionSchema,
  contextSchema,
  deviceSchema,
  logLimitsSchema,
  logScopeSchema,
  networkPolicySchema,
  startRequestSchema,
  type StartRequest,
} from "./product";

const coverageSchema = z.object({
  decoded_http_events: z.number(),
  tls_errors: z.number(),
  omitted_flows: z.number(),
  unavailable_bodies: z.number(),
  admission_closed: z.boolean(),
  public_certificate: z.string().nullable(),
  synthetic_check_url: z.string().nullable(),
});
const completionSchema = z.object({
  coverage: coverageSchema,
  routing_restored: z.boolean(),
  partial: z.boolean(),
  forced: z.boolean(),
  interrupted: z.boolean(),
  reasons: z.array(z.string()),
});
const networkSchema = z.discriminatedUnion("state", [
  z.object({ state: z.literal("not_selected") }),
  z.object({ state: z.literal("preparing") }),
  z.object({ state: z.literal("recording"), detail: coverageSchema }),
  z.object({
    state: z.literal("recovery_required"),
    detail: z.object({
      message: z.string(),
      guardian_interrupted: z.boolean(),
    }),
  }),
  z.object({ state: z.literal("stopped"), detail: completionSchema }),
  z.object({ state: z.literal("interrupted"), detail: completionSchema }),
]);
const receiptSchema = z.object({
  reason: z.string(),
  bytes: z.number(),
  records: z.number(),
  incomplete_tail: z.boolean(),
  child_reaped: z.boolean(),
  forced: z.boolean(),
  exit_code: z.number().nullable(),
  stderr: z.string(),
  ended_at: z.string(),
});
const attributionSchema = z.object({
  strategy: z.enum(["whole_device", "android_uid", "android_process_map"]),
  application_id: z.string().nullable(),
  user_id: z.number().nullable(),
  uid: z.number().nullable(),
  install_identity: z.string().nullable(),
  levels: z.string(),
  buffers: z.array(z.string()),
  exclusions: z.array(z.string()),
});
const sessionSchema = z.object({
  id: z.string(),
  device: deviceSchema,
  directory: z.string(),
  started_at: z.string(),
  limits: logLimitsSchema,
  fixture: z.boolean(),
  bytes: z.number(),
  logs: z.number(),
  markers: z.number(),
  receipt: receiptSchema.nullable(),
  archive: z.string().nullable(),
  warnings: z.array(z.string()),
  network: networkSchema,
  descriptor_version: z.number(),
  selection: captureSelectionSchema,
  log_scope: logScopeSchema,
  network_policy: networkPolicySchema,
  context: contextSchema,
  log_attribution: attributionSchema.nullable(),
});
export const snapshotSchema = z.discriminatedUnion("phase", [
  z.object({
    phase: z.literal("idle"),
    session: z.object({ fixture: z.boolean(), limits: logLimitsSchema }),
  }),
  z.object({ phase: z.literal("recording"), session: sessionSchema }),
  z.object({ phase: z.literal("stopped"), session: sessionSchema }),
  z.object({ phase: z.literal("failed"), session: sessionSchema }),
  z.object({ phase: z.literal("unverified_shutdown"), session: sessionSchema }),
  z.object({ phase: z.literal("recovery_required"), session: sessionSchema }),
]);
export const preferencesSchema = z.object({
  output_root: z.string(),
  limits: logLimitsSchema,
  network_policy: networkPolicySchema,
  adb_override: z.string().nullable(),
  mitmdump_override: z.string().nullable(),
});
const operationKindSchema = z.enum([
  "save",
  "certificate",
  "connection_check",
  "readiness_report",
]);
export const certificateResultSchema = z.object({
  kind: z.literal("certificate"),
  serial: z.string(),
  certificate_fingerprint: z.string(),
  certificate_path: z.string(),
  device_path: z.string(),
  routing_restored: z.boolean(),
});
export const checkResultSchema = z.object({
  kind: z.literal("connection_check"),
  serial: z.string(),
  directory: z.string(),
  url: z.string(),
  certificate_fingerprint: z.string(),
  https_observed: z.boolean(),
  phone_origin_confirmed: z.boolean(),
  routing_restored: z.boolean(),
  verified: z.boolean(),
  checked_at: z.string(),
  expires_at: z.string(),
  message: z.string(),
});
const resultSchema = z.discriminatedUnion("kind", [
  z.object({
    kind: z.literal("save"),
    archive: z.string(),
    directory: z.string(),
  }),
  certificateResultSchema,
  checkResultSchema,
  z.object({ kind: z.literal("readiness_report"), path: z.string() }),
]);
export const operationSchema = z.discriminatedUnion("state", [
  z.object({ state: z.literal("idle") }),
  z.object({
    state: z.literal("saving"),
    id: z.string(),
    stage: z.string(),
    completed: z.number(),
    total: z.number().nullable(),
    cancel_allowed: z.boolean(),
  }),
  z.object({
    state: z.literal("preparing_certificate"),
    id: z.string(),
    stage: z.string(),
    message: z.string(),
  }),
  z.object({
    state: z.literal("checking_connection"),
    id: z.string(),
    url: z.string(),
    certificate_fingerprint: z.string(),
    https_observed: z.boolean(),
    phone_origin_confirmed: z.boolean(),
    remaining_seconds: z.number(),
  }),
  z.object({
    state: z.literal("restoring"),
    id: z.string(),
    kind: operationKindSchema,
    message: z.string(),
  }),
  z.object({
    state: z.literal("finished"),
    id: z.string(),
    result: resultSchema,
  }),
  z.object({
    state: z.literal("failed"),
    id: z.string(),
    kind: operationKindSchema,
    message: z.string(),
    recovery_required: z.boolean(),
    directory: z.string().nullable(),
  }),
  z.object({
    state: z.literal("cancelled"),
    id: z.string(),
    kind: operationKindSchema,
    message: z.string(),
  }),
]);
export const doctorSchema = z.object({
  schemaVersion: z.number(),
  command: z.string(),
  scope: z.string(),
  platform: z.string(),
  checkedAt: z.string(),
  hostTarget: z.string(),
  hostPrerequisitesReady: z.boolean(),
  sources: z.enum(["logs", "network", "logsAndNetwork"]),
  checks: z.array(
    z.object({
      id: z.string(),
      status: z.enum(["ready", "needsAction", "missing", "incompatible"]),
      message: z.string(),
      source: z.string(),
      path: z.string().nullable(),
      version: z.string().nullable(),
    }),
  ),
  deviceSetup: z.object({
    status: z.string(),
    manualActions: z.array(z.string()),
  }),
  error: z.object({ code: z.string(), message: z.string() }).nullable(),
});
export const setupIdentitySchema = z.object({
  certificate_fingerprint: z.string().nullable(),
  certificate_path: z.string().nullable(),
  checked_at: z.string(),
});
export type Snapshot = z.infer<typeof snapshotSchema>;
export type Session = z.infer<typeof sessionSchema>;
export type Device = z.infer<typeof deviceSchema>;
export type Preferences = z.infer<typeof preferencesSchema>;
export type Operation = z.infer<typeof operationSchema>;
export type DoctorReport = z.infer<typeof doctorSchema>;
export type CertificateResult = z.infer<typeof certificateResultSchema>;
export type CheckResult = z.infer<typeof checkResultSchema>;
export type SetupIdentity = z.infer<typeof setupIdentitySchema>;
export const api = {
  discover: async () =>
    z.array(deviceSchema).parse(await invoke<unknown>("discover")),
  snapshot: async () => snapshotSchema.parse(await invoke<unknown>("snapshot")),
  start: async (request: StartRequest) =>
    snapshotSchema.parse(
      await invoke<unknown>("start", {
        request: startRequestSchema.parse(request),
      }),
    ),
  marker: async (text: string) =>
    snapshotSchema.parse(await invoke<unknown>("marker", { text })),
  stop: async () => snapshotSchema.parse(await invoke<unknown>("stop")),
  recover: async () => snapshotSchema.parse(await invoke<unknown>("recover")),
  preferences: async () =>
    preferencesSchema.parse(await invoke<unknown>("preferences")),
  savePreferences: async (preferences: Preferences) =>
    preferencesSchema.parse(
      await invoke<unknown>("save_preferences", {
        preferences: preferencesSchema.parse(preferences),
      }),
    ),
  doctor: async (sources: DoctorReport["sources"]) =>
    doctorSchema.parse(await invoke<unknown>("doctor", { sources })),
  operation: async () =>
    operationSchema.parse(await invoke<unknown>("operation")),
  save: async (directory: string | null) =>
    operationSchema.parse(await invoke<unknown>("begin_save", { directory })),
  certificate: async (serial: string) =>
    operationSchema.parse(
      await invoke<unknown>("begin_certificate", { serial }),
    ),
  check: async (serial: string) =>
    operationSchema.parse(
      await invoke<unknown>("begin_connection_check", { serial }),
    ),
  confirmOrigin: async (id: string, confirmed: boolean) =>
    operationSchema.parse(
      await invoke<unknown>("confirm_check_origin", { id, confirmed }),
    ),
  cancel: async (id: string) =>
    operationSchema.parse(await invoke<unknown>("cancel_operation", { id })),
  retryRecovery: async (id: string) =>
    operationSchema.parse(
      await invoke<unknown>("retry_setup_recovery", { id }),
    ),
  openCheck: async (id: string) => {
    await invoke<unknown>("open_check_on_phone", { id });
  },
  identity: async () =>
    setupIdentitySchema.parse(await invoke<unknown>("setup_identity")),
  copyArchive: async () =>
    z
      .string()
      .nullable()
      .parse(await invoke<unknown>("copy_archive")),
  report: async () =>
    z
      .string()
      .nullable()
      .parse(await invoke<unknown>("export_readiness_report")),
  updates: async () => {
    await invoke<unknown>("open_updates");
  },
};
export function operationBusy(operation: Operation): boolean {
  return (
    operation.state === "saving" ||
    operation.state === "preparing_certificate" ||
    operation.state === "checking_connection" ||
    operation.state === "restoring" ||
    (operation.state === "failed" && operation.recovery_required)
  );
}
export function captureSafe(snapshot: Snapshot | undefined): boolean {
  if (
    !snapshot ||
    snapshot.phase === "recording" ||
    snapshot.phase === "recovery_required" ||
    snapshot.phase === "unverified_shutdown"
  )
    return false;
  if (snapshot.phase === "idle") return true;
  const network = snapshot.session.network;
  return (
    network.state === "not_selected" ||
    ((network.state === "stopped" || network.state === "interrupted") &&
      network.detail.routing_restored)
  );
}
export function errorMessage(cause: unknown): string {
  return cause instanceof Error ? cause.message : String(cause);
}
