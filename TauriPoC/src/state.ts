import { useEffect, useState } from "react";
import { z } from "zod";
import {
  checkResultSchema,
  certificateResultSchema,
  type SetupIdentity,
  type CheckResult,
} from "./api";
import { contextSchema } from "./product";

export const draftSchema = z.object({
  serial: z.string(),
  selection: z.enum(["network", "logs", "logs_and_network"]),
  log_mode: z.enum(["app", "whole_device"]),
  application_id: z.string(),
  context: contextSchema,
});
const readingSchema = z.object({
  wrap: z.boolean(),
  follow: z.boolean(),
  order: z.enum(["oldest_first", "newest_first"]),
});
export const setupHistorySchema = z.object({
  deferred: z.boolean(),
  certificate: certificateResultSchema.nullable(),
  check: checkResultSchema.nullable(),
});
export type CaptureDraft = z.infer<typeof draftSchema>;
export type SetupHistory = z.infer<typeof setupHistorySchema>;
export type RunAction = (
  label: string,
  action: () => Promise<void>,
) => Promise<void>;
export const initialDraft: CaptureDraft = {
  serial: "",
  selection: "network",
  log_mode: "app",
  application_id: "",
  context: { title: "", notes: "", steps: [] },
};
export const initialReading: z.infer<typeof readingSchema> = {
  wrap: true,
  follow: true,
  order: "newest_first",
};
export const initialHistory: SetupHistory = {
  deferred: false,
  certificate: null,
  check: null,
};
export function useStored<T>(key: string, schema: z.ZodType<T>, fallback: T) {
  const [value, setValue] = useState<T>(() => {
    try {
      const stored = localStorage.getItem(key);
      return stored ? schema.parse(JSON.parse(stored)) : fallback;
    } catch {
      return fallback;
    }
  });
  useEffect(() => {
    try {
      localStorage.setItem(key, JSON.stringify(value));
    } catch {
    }
  }, [key, value]);
  const result: [T, typeof setValue] = [value, setValue];
  return result;
}
export const useDraft = () =>
  useStored("android.capture.draft.v1", draftSchema, initialDraft);
export const useReading = () =>
  useStored("android.capture.reading.v1", readingSchema, initialReading);
export const useSetupHistory = () =>
  useStored("android.capture.setup.v1", setupHistorySchema, initialHistory);
export function checkFreshness(
  check: CheckResult | null,
  serial: string,
  identity: SetupIdentity | undefined,
  now = Date.now(),
): string {
  if (!check) return "No connection check has completed.";
  if (check.serial !== serial)
    return "This check belongs to a different device.";
  if (!identity?.certificate_fingerprint)
    return "The current capture certificate has not been confirmed.";
  if (identity.certificate_fingerprint !== check.certificate_fingerprint)
    return "The capture certificate changed. Prepare and check again.";
  if (
    !Number.isFinite(Date.parse(check.expires_at)) ||
    now >= Date.parse(check.expires_at)
  )
    return "This connection check expired. Run a fresh check.";
  if (!check.routing_restored) return "Device routing still needs recovery.";
  if (!check.verified || !check.https_observed || !check.phone_origin_confirmed)
    return check.message;
  return "";
}
export function timelineFence(
  action: "pause" | "clear" | "full",
  latest: number,
  previous: { after: number | null; through: number | null },
) {
  switch (action) {
    case "pause":
      return { after: previous.after, through: latest };
    case "clear":
      return { after: latest, through: null };
    case "full":
      return { after: null, through: null };
  }
}
