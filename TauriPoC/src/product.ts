import { invoke } from "@tauri-apps/api/core";
import { z } from "zod";

export const logScopeSchema = z.discriminatedUnion("mode", [
  z.object({ mode: z.literal("whole_device") }),
  z.object({ mode: z.literal("app"), application_id: z.string() }),
]);
export const logLimitsSchema = z.object({ bytes: z.number(), records: z.number(), duration_ms: z.number() });
export const networkPolicySchema = z.object({
  max_flows: z.number(), max_body_bytes: z.number(), max_header_bytes: z.number(),
  max_total_bytes: z.number(), duration_ms: z.number(),
});
export const contextSchema = z.object({ title: z.string(), notes: z.string(), steps: z.array(z.string()) });
export const captureSelectionSchema = z.discriminatedUnion("mode", [
  z.object({ mode: z.literal("logs") }),
  z.object({ mode: z.literal("network"), consent: z.object({ whole_device: z.boolean() }) }),
  z.object({ mode: z.literal("logs_and_network"), consent: z.object({ whole_device: z.boolean() }) }),
]);
export const startRequestSchema = z.object({
  serial: z.string(), whole_device_consent: z.boolean(), selection: captureSelectionSchema,
  log_scope: logScopeSchema, limits: logLimitsSchema, network_policy: networkPolicySchema, context: contextSchema,
});
export const applicationSchema = z.object({ application_id: z.string(), label: z.string(), user_id: z.number(), uid: z.number(), shared_uid: z.boolean() });
export const deviceSchema = z.object({ serial: z.string(), name: z.string(), state: z.enum(["ready", "offline", "unauthorized", "unavailable"]) });
export const eventSchema = z.object({
  id: z.string(), timestamp: z.string(), original_timestamp: z.string().nullable(), received_at: z.string().nullable(),
  source: z.string(), kind: z.string(), summary: z.string(), raw_ref: z.string(),
  detail: z.record(z.string(), z.unknown()), clock_uncertainty_ms: z.number().nullable(),
});
export const progressSchema = z.object({
  indexed: z.number(), raw_bytes: z.number(), indexed_bytes: z.number(), backlog_bytes: z.number(),
  complete: z.boolean(), latest_sequence: z.number(),
});
export const timelineQuerySchema = z.object({
  source: z.string().nullable(), search: z.string(), order: z.enum(["oldest_first", "newest_first"]),
  limit: z.number(), cursor: z.string().nullable(), after_sequence: z.number().nullable(), through_sequence: z.number().nullable(),
});
export const pageSchema = z.object({
  generation: z.string(), rows: z.array(z.object({ sequence: z.number(), event: eventSchema })),
  next_cursor: z.string().nullable(), total: z.number(), matching: z.number(), progress: progressSchema,
});
const bodySideSchema = z.enum(["request", "response"]);
export const bodyDescriptorSchema = z.object({
  side: bodySideSchema, available: z.boolean(), reason: z.string().nullable(), bytes: z.number(), text: z.boolean(),
  byte_basis: z.string(), content_encoding: z.string().nullable(), decoded_available: z.boolean(), decoded_reason: z.string().nullable(),
});
export const dataChunkSchema = z.object({ offset: z.number(), total_bytes: z.number(), data_base64: z.string(), next_offset: z.number().nullable() });
export const detailSchema = z.object({ sequence: z.number(), event: eventSchema, detail_bytes: z.number(), detail_complete: z.boolean(), bodies: z.array(bodyDescriptorSchema) });
export const bodyChunkSchema = z.object({ body: bodyDescriptorSchema, chunk: dataChunkSchema });
export const descriptorSchema = z.object({
  version: z.number(), id: z.string(), directory: z.string(), device: deviceSchema.nullable(), started_at: z.string(),
  legacy: z.boolean(), metadata: z.unknown(), warnings: z.array(z.string()),
});
export const catalogSchema = z.object({ sessions: z.array(descriptorSchema), scanned: z.number(), truncated: z.boolean(), warnings: z.array(z.string()) });

export type LogScope = z.infer<typeof logScopeSchema>;
export type LogLimits = z.infer<typeof logLimitsSchema>;
export type NetworkPolicy = z.infer<typeof networkPolicySchema>;
export type CaptureContext = z.infer<typeof contextSchema>;
export type StartRequest = z.infer<typeof startRequestSchema>;
export type Application = z.infer<typeof applicationSchema>;
export type TimelineQuery = z.infer<typeof timelineQuerySchema>;
export type TimelinePage = z.infer<typeof pageSchema>;
export type EventDetail = z.infer<typeof detailSchema>;
export type BodyDescriptor = z.infer<typeof bodyDescriptorSchema>;
export type BodySide = z.infer<typeof bodySideSchema>;
export type SessionDescriptor = z.infer<typeof descriptorSchema>;

export const product = {
  applications: async (serial: string) => z.array(applicationSchema).parse(await invoke<unknown>("applications", { serial })),
  catalog: async () => catalogSchema.parse(await invoke<unknown>("sessions")),
  open: async (directory: string) => descriptorSchema.parse(await invoke<unknown>("open_session", { directory })),
  query: async (query: TimelineQuery) => pageSchema.parse(await invoke<unknown>("timeline_query", { query })),
  detail: async (id: string) => detailSchema.nullable().parse(await invoke<unknown>("event_detail", { id })),
  body: async (id: string, side: BodySide, offset: number, decoded = false) => bodyChunkSchema.parse(await invoke<unknown>("body_chunk", { id, side, offset, limit: 65536, decoded })),
  detailChunk: async (id: string, offset: number) => dataChunkSchema.parse(await invoke<unknown>("detail_chunk", { id, offset, limit: 65536 })),
  chooseFolder: async () => z.string().nullable().parse(await invoke<unknown>("choose_folder")),
  reveal: async (target: "session" | "raw" | "archive" | "certificate", id: string | null = null) => {
    await invoke<unknown>("reveal", { target, id });
  },
};

export function decodeChunk(data: string): Uint8Array {
  return Uint8Array.from(atob(data), (character) => character.charCodeAt(0));
}

export function formatBytes(bytes: number): string {
  if (bytes < 1000) return `${bytes} B`;
  if (bytes < 1000000) return `${(bytes / 1000).toFixed(1)} KB`;
  if (bytes < 1000000000) return `${(bytes / 1000000).toFixed(1)} MB`;
  return `${(bytes / 1000000000).toFixed(1)} GB`;
}
