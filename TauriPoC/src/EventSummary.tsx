import { Badge } from "./components/ui/badge";
import type { EventDetail } from "./product";
import { logLevel, methodColor } from "./event-presentation";
export function EventSummary({ event }: { event: EventDetail["event"] }) {
  const fields = event.detail;
  if (event.source === "logs") {
    const level = logLevel(fields.level);
    return (
      <span className="flex min-w-0 flex-col gap-1">
        <span className="flex flex-wrap items-center gap-2">
          <Badge className={level.color}>{level.label}</Badge>
          {typeof fields.tag === "string" && (
            <span className="text-xs text-muted-foreground">{fields.tag}</span>
          )}
        </span>
        <span className="event-summary text-sm leading-5">
          {typeof fields.message === "string" ? fields.message : event.summary}
        </span>
      </span>
    );
  }
  if (event.source === "network") {
    const method = typeof fields.method === "string" ? fields.method : "HTTP";
    const status = typeof fields.status === "number" ? fields.status : null;
    const duration =
      typeof fields.duration_ms === "number" &&
      Number.isFinite(fields.duration_ms) &&
      fields.duration_ms >= 0
        ? fields.duration_ms
        : null;
    return (
      <span className="flex min-w-0 flex-col gap-1">
        <span className="flex flex-wrap items-center gap-2 text-xs">
          <Badge className={methodColor(method)}>{method}</Badge>
          {status !== null && (
            <Badge
              className={
                status >= 400
                  ? "bg-red-500/10 text-red-700 dark:text-red-300"
                  : "bg-muted text-muted-foreground"
              }
            >
              Status {status}
            </Badge>
          )}
          {duration !== null && (
            <span className="text-muted-foreground">
              Duration{" "}
              {duration < 1000
                ? `${Math.round(duration)} ms`
                : `${(duration / 1000).toFixed(2)} s`}
            </span>
          )}
        </span>
        <span className="event-summary text-sm leading-5">{event.summary}</span>
      </span>
    );
  }
  return (
    <span className="event-summary min-w-0 text-sm leading-5">
      {event.summary}
    </span>
  );
}
