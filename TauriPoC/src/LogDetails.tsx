import { useEffect, useState } from "react";
import { eventSchema, product, decodeChunk, type EventDetail } from "./product";
import { errorMessage } from "./api";
import { BodyViewport } from "./BodyViewport";
import { logLevel } from "./event-presentation";
import { Badge } from "./components/ui/badge";

type State =
  | { kind: "loading" }
  | { kind: "ready"; fields: EventDetail["event"]["detail"] }
  | { kind: "error"; message: string };
export function LogDetails({
  detail,
  wrap,
}: {
  detail: EventDetail;
  wrap: boolean;
}) {
  const [state, setState] = useState<State>(
    detail.detail_complete
      ? { kind: "ready", fields: detail.event.detail }
      : { kind: "loading" },
  );
  const [offset, setOffset] = useState(0);
  useEffect(() => {
    if (detail.detail_complete) return;
    let disposed = false;
    async function load() {
      const decoder = new TextDecoder("utf-8", { fatal: true });
      const parts: string[] = [];
      let next: number | null = 0;
      while (next !== null && !disposed) {
        const chunk = await product.detailChunk(
          { session_key: detail.session_key, event_id: detail.event.id },
          next,
        );
        parts.push(
          decoder.decode(decodeChunk(chunk.data_base64), {
            stream: chunk.next_offset !== null,
          }),
        );
        next = chunk.next_offset;
      }
      if (!disposed)
        setState({
          kind: "ready",
          fields: eventSchema.shape.detail.parse(JSON.parse(parts.join(""))),
        });
    }
    void load().catch((cause: unknown) => {
      if (!disposed) setState({ kind: "error", message: errorMessage(cause) });
    });
    return () => {
      disposed = true;
    };
  }, [detail]);
  if (state.kind === "loading")
    return <p role="status">Reading the complete log message…</p>;
  if (state.kind === "error")
    return (
      <p role="alert">Full log message could not be read. {state.message}</p>
    );
  const fields = state.fields;
  const level = logLevel(fields.level);
  const message =
    typeof fields.message === "string" ? fields.message : detail.event.summary;
  return (
    <div className="space-y-3">
      <Badge className={level.color}>{level.label}</Badge>
      <BodyViewport
        text={message}
        offset={offset}
        onOffset={setOffset}
        wrap={wrap}
      />
      <details className="text-xs [&_dt]:text-muted-foreground [&_dd]:break-all [&_dd]:font-mono">
        <summary className="cursor-pointer py-2">Log metadata</summary>
        <dl className="grid gap-2">
          {["tag", "pid", "tid"].map((key) => (
            <div key={key}>
              <dt>{key.toUpperCase()}</dt>
              <dd>
                {typeof fields[key] === "string" ||
                typeof fields[key] === "number"
                  ? String(fields[key])
                  : "Unavailable"}
              </dd>
            </div>
          ))}
        </dl>
      </details>
    </div>
  );
}
