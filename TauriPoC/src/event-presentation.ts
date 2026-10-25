import { logLevelSchema, type LogLevel } from "./product";
const levels = {
  V: { label: "Verbose", color: "bg-muted text-muted-foreground" },
  D: { label: "Debug", color: "bg-sky-500/10 text-sky-700 dark:text-sky-300" },
  I: { label: "Info", color: "bg-muted text-foreground" },
  W: {
    label: "Warning",
    color: "bg-amber-500/10 text-amber-700 dark:text-amber-300",
  },
  E: { label: "Error", color: "bg-red-500/10 text-red-700 dark:text-red-300" },
  F: { label: "Fatal", color: "bg-red-500/10 text-red-700 dark:text-red-300" },
  A: { label: "Assert", color: "bg-red-500/10 text-red-700 dark:text-red-300" },
  S: { label: "Silent", color: "bg-muted text-muted-foreground" },
  unknown: { label: "Unknown", color: "bg-muted text-muted-foreground" },
} satisfies Record<LogLevel, { label: string; color: string }>;
export function logLevel(value: unknown) {
  const parsed = logLevelSchema.safeParse(value);
  return levels[parsed.success ? parsed.data : "unknown"];
}
export function methodColor(method: string) {
  switch (method) {
    case "GET":
    case "HEAD":
      return "bg-sky-500/10 text-sky-700 dark:text-sky-300";
    case "POST":
      return "bg-emerald-500/10 text-emerald-700 dark:text-emerald-300";
    case "PUT":
    case "PATCH":
      return "bg-amber-500/10 text-amber-700 dark:text-amber-300";
    case "DELETE":
      return "bg-red-500/10 text-red-700 dark:text-red-300";
    default:
      return "bg-muted text-muted-foreground";
  }
}
