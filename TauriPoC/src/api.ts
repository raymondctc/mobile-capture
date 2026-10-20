import { invoke } from "@tauri-apps/api/core";
import { z } from "zod";

const device = z.object({
  serial: z.string(),
  name: z.string(),
  state: z.enum(["ready", "offline", "unauthorized", "unavailable"]),
});
const limits = z.object({
  bytes: z.number(),
  records: z.number(),
  duration_ms: z.number(),
});
const event = z.object({
  id: z.string(),
  timestamp: z.string(),
  original_timestamp: z.string().nullable(),
  received_at: z.string().nullable(),
  source: z.enum(["logs", "marker", "network"]),
  kind: z.string(),
  summary: z.string(),
  raw_ref: z.string(),
  detail: z.record(z.string(), z.unknown()),
  clock_uncertainty_ms: z.number().nullable(),
});
const receipt = z.object({
  reason: z.enum([
    "lease_closed",
    "duration_limit",
    "byte_limit",
    "record_limit",
    "collector_exit",
    "read_error",
  ]),
  bytes: z.number(),
  records: z.number(),
  incomplete_tail: z.boolean(),
  child_reaped: z.boolean(),
  forced: z.boolean(),
  exit_code: z.number().nullable(),
  stderr: z.string(),
  ended_at: z.string(),
});
const coverage = z.object({
  decoded_http_events: z.number(),
  tls_errors: z.number(),
  omitted_flows: z.number(),
  unavailable_bodies: z.number(),
  admission_closed: z.boolean(),
  public_certificate: z.string().nullable(),
  synthetic_check_url: z.string().nullable(),
});
const completion = z.object({
  coverage,
  routing_restored: z.boolean(),
  partial: z.boolean(),
  forced: z.boolean(),
  interrupted: z.boolean(),
  reasons: z.array(z.string()),
});
const network = z.discriminatedUnion("state", [
  z.object({ state: z.literal("not_selected") }),
  z.object({ state: z.literal("preparing") }),
  z.object({ state: z.literal("recording"), detail: coverage }),
  z.object({
    state: z.literal("recovery_required"),
    detail: z.object({ message: z.string(), guardian_interrupted: z.boolean() }),
  }),
  z.object({ state: z.literal("stopped"), detail: completion }),
  z.object({ state: z.literal("interrupted"), detail: completion }),
]);
const session = z.object({
  id: z.string(),
  device,
  directory: z.string(),
  started_at: z.string(),
  limits,
  fixture: z.boolean(),
  bytes: z.number(),
  logs: z.number(),
  markers: z.number(),
  events: z.array(event),
  receipt: receipt.nullable(),
  archive: z.string().nullable(),
  warnings: z.array(z.string()),
  network,
});
const snapshot = z.discriminatedUnion("phase", [
  z.object({
    phase: z.literal("idle"),
    session: z.object({ fixture: z.boolean(), limits }),
  }),
  z.object({ phase: z.literal("recording"), session }),
  z.object({ phase: z.literal("stopped"), session }),
  z.object({ phase: z.literal("failed"), session }),
  z.object({ phase: z.literal("unverified_shutdown"), session }),
  z.object({ phase: z.literal("recovery_required"), session }),
]);
export type Device = z.infer<typeof device>;
export type Event = z.infer<typeof event>;
export type Snapshot = z.infer<typeof snapshot>;
export type NetworkView = z.infer<typeof network>;
const bodyPreview = z.union([
  z.object({
    encoding: z.literal("base64"),
    text: z.string(),
    bytes: z.number(),
    byte_basis: z.literal("raw_content"),
    preview: z.literal(true),
    preview_bytes: z.number(),
    retained_bytes: z.number(),
  }),
  z.object({
    encoding: z.null(),
    text: z.null(),
    bytes: z.null(),
    unavailable: z.literal(true),
    reason: z.string(),
    byte_basis: z.literal("raw_content"),
  }),
]);
export type BodyPreview = z.infer<typeof bodyPreview>;
export function bodyPreviews(detail: Event["detail"]): { label: string; body: BodyPreview }[] {
  const found: { label: string; body: BodyPreview }[] = [];
  for (const label of ["request_body", "response_body"]) {
    const parsed = bodyPreview.safeParse(detail[label]);
    if (parsed.success) found.push({ label, body: parsed.data });
  }
  return found;
}
export const api = {
  discover: async () =>
    z.array(device).parse(await invoke<unknown>("discover")),
  snapshot: async () => snapshot.parse(await invoke<unknown>("snapshot")),
  start: async (input: { serial: string; wholeDeviceConsent: boolean; networkConsent: boolean }) =>
    snapshot.parse(await invoke<unknown>("start", input)),
  marker: async (input: { text: string }) =>
    snapshot.parse(await invoke<unknown>("marker", input)),
  stop: async () => snapshot.parse(await invoke<unknown>("stop")),
  recover: async () => snapshot.parse(await invoke<unknown>("recover")),
  export: async () => z.string().parse(await invoke<unknown>("export")),
};
