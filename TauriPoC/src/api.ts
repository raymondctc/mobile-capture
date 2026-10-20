import { invoke } from "@tauri-apps/api/core";
import { z } from "zod";

const device = z.object({ serial: z.string(), name: z.string(), state: z.enum(["ready", "offline", "unauthorized", "unavailable"]) });
const limits = z.object({ bytes: z.number(), records: z.number(), duration_ms: z.number() });
const event = z.object({ id: z.string(), timestamp: z.string(), original_timestamp: z.string().nullable(), received_at: z.string().nullable(), source: z.enum(["logs", "marker"]), kind: z.string(), summary: z.string(), raw_ref: z.string(), detail: z.record(z.string(), z.unknown()), clock_uncertainty_ms: z.number().nullable() });
const receipt = z.object({ reason: z.enum(["lease_closed", "duration_limit", "byte_limit", "record_limit", "collector_exit", "read_error"]), bytes: z.number(), records: z.number(), incomplete_tail: z.boolean(), child_reaped: z.boolean(), forced: z.boolean(), exit_code: z.number().nullable(), stderr: z.string(), ended_at: z.string() });
const session = z.object({ id: z.string(), device, directory: z.string(), started_at: z.string(), limits, fixture: z.boolean(), bytes: z.number(), logs: z.number(), markers: z.number(), events: z.array(event), receipt: receipt.nullable(), archive: z.string().nullable(), warnings: z.array(z.string()) });
const snapshot = z.discriminatedUnion("phase", [z.object({ phase: z.literal("idle"), session: z.object({ fixture: z.boolean(), limits }) }), z.object({ phase: z.literal("recording"), session }), z.object({ phase: z.literal("stopped"), session }), z.object({ phase: z.literal("failed"), session }), z.object({ phase: z.literal("unverified_shutdown"), session })]);
export type Device = z.infer<typeof device>;
export type Event = z.infer<typeof event>;
export type Snapshot = z.infer<typeof snapshot>;
export const api = {
  discover: async () => z.array(device).parse(await invoke<unknown>("discover")),
  snapshot: async () => snapshot.parse(await invoke<unknown>("snapshot")),
  start: async (input: { serial: string; wholeDeviceConsent: boolean }) => snapshot.parse(await invoke<unknown>("start", input)),
  marker: async (input: { text: string }) => snapshot.parse(await invoke<unknown>("marker", input)),
  stop: async () => snapshot.parse(await invoke<unknown>("stop")),
  export: async () => z.string().parse(await invoke<unknown>("export")),
};
