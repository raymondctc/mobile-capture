import { Badge } from "./components/ui/badge";
import { Checkbox } from "./components/ui/checkbox";
import { Button } from "./components/ui/button";
import { Label } from "./components/ui/label";
import { useEffect, useRef, useState } from "react";
import {
  decodeChunk,
  formatBytes,
  product,
  type BodyDescriptor,
  type EventDetail,
  type BodySide,
  type dataChunkSchema,
} from "./product";
import type { z } from "zod";
import { errorMessage } from "./api";
import { timeLabel } from "./ui";

type Chunk = z.infer<typeof dataChunkSchema>;
type ContentView =
  | { kind: "metadata" }
  | { kind: "body"; side: BodySide; decoded: boolean; binary: boolean };
type RevealState =
  { kind: "idle" } | { kind: "pending" } | { kind: "error"; message: string };
export function hexChunk(bytes: Uint8Array, offset: number): string {
  const lines: string[] = [];
  for (let index = 0; index < bytes.length; index += 16) {
    const row = bytes.slice(index, index + 16);
    const hex = Array.from(row, (byte) => byte.toString(16).padStart(2, "0"))
      .join(" ")
      .padEnd(47, " ");
    const text = Array.from(row, (byte) =>
      byte >= 32 && byte < 127 ? String.fromCharCode(byte) : ".",
    ).join("");
    lines.push(
      `${(offset + index).toString(16).padStart(8, "0")}  ${hex}  ${text}`,
    );
  }
  return lines.join("\n");
}
export function Details({
  detail,
  offPage,
  wrap,
}: {
  detail: EventDetail;
  offPage: boolean;
  wrap: boolean;
}) {
  const [view, setView] = useState<ContentView>({ kind: "metadata" });
  const [chunk, setChunk] = useState<Chunk>();
  const [offset, setOffset] = useState(0);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [reveal, setReveal] = useState<RevealState>({ kind: "idle" });
  const generation = useRef(0);
  const revealGeneration = useRef(0);
  useEffect(() => {
    return () => {
      revealGeneration.current += 1;
    };
  }, []);
  useEffect(() => {
    const request = ++generation.current;
    const id = detail.event.id;
    setBusy(true);
    setError("");
    const load =
      view.kind === "metadata"
        ? product.detailChunk(id, offset)
        : product
            .body(id, view.side, offset, view.decoded)
            .then((result) => result.chunk);
    void load
      .then((result) => {
        if (request === generation.current) setChunk(result);
      })
      .catch((cause: unknown) => {
        if (request === generation.current) {
          setError(errorMessage(cause));
          setChunk(undefined);
        }
      })
      .finally(() => {
        if (request === generation.current) setBusy(false);
      });
    return () => {
      generation.current += 1;
    };
  }, [detail.event.id, view, offset]);
  async function revealRaw() {
    const request = ++revealGeneration.current;
    setReveal({ kind: "pending" });
    try {
      await product.reveal("raw", detail.event.id);
      if (request === revealGeneration.current) setReveal({ kind: "idle" });
    } catch (cause) {
      if (request === revealGeneration.current)
        setReveal({ kind: "error", message: errorMessage(cause) });
    }
  }
  function showBody(body: BodyDescriptor, decoded: boolean) {
    setChunk(undefined);
    setOffset(0);
    setView({
      kind: "body",
      side: body.side,
      decoded,
      binary: !body.text && !decoded,
    });
  }
  let text = "";
  if (chunk) {
    const bytes = decodeChunk(chunk.data_base64);
    text =
      view.kind === "body" && view.binary
        ? hexChunk(bytes, chunk.offset)
        : new TextDecoder("utf-8").decode(bytes);
  }
  return (
    <section
      className="space-y-4 select-text"
      aria-label="Event details"
      aria-busy={busy}
    >
      <div className="flex flex-wrap items-center justify-between gap-2">
        {offPage && (
          <Badge variant="secondary">Selected outside this page</Badge>
        )}
        <Button
          variant="ghost"
          disabled={reveal.kind === "pending"}
          onClick={() => void revealRaw()}
        >
          {reveal.kind === "pending"
            ? "Revealing raw source…"
            : "Reveal raw source"}
        </Button>
      </div>
      {reveal.kind === "error" && (
        <p
          className="rounded-md border border-warning/30 bg-warning/10 px-3 py-2 text-xs leading-relaxed text-warning select-text"
          role="alert"
        >
          {reveal.message}
        </p>
      )}
      <dl className="grid gap-3 text-xs [&_dt]:mb-1 [&_dt]:text-muted-foreground [&_dd]:break-all [&_dd]:font-mono [&_dd]:select-text">
        <div>
          <dt>Time</dt>
          <dd>{timeLabel(detail.event.timestamp)}</dd>
        </div>
        <div>
          <dt>Source</dt>
          <dd>
            {detail.event.source} · {detail.event.kind}
          </dd>
        </div>
        <div>
          <dt>Raw reference</dt>
          <dd>{detail.event.raw_ref}</dd>
        </div>
        <div>
          <dt>Event ID</dt>
          <dd>{detail.event.id}</dd>
        </div>
        {detail.event.clock_uncertainty_ms !== null && (
          <div>
            <dt>Clock uncertainty</dt>
            <dd>{detail.event.clock_uncertainty_ms} ms</dd>
          </div>
        )}
        {detail.event.original_timestamp && (
          <div>
            <dt>Original time</dt>
            <dd>{detail.event.original_timestamp}</dd>
          </div>
        )}
        {detail.event.received_at && (
          <div>
            <dt>Received</dt>
            <dd>{detail.event.received_at}</dd>
          </div>
        )}
      </dl>
      <p className="break-words text-sm leading-relaxed select-text">
        {detail.event.summary}
      </p>
      <div className="flex flex-wrap gap-2 [&_button]:h-auto [&_button]:min-h-8 [&_button]:whitespace-normal [&_button]:text-left [&_button[aria-pressed=true]]:border-primary [&_button[aria-pressed=true]]:bg-accent [&_button[aria-pressed=true]]:text-accent-foreground">
        <Button
          aria-pressed={view.kind === "metadata"}
          onClick={() => {
            setView({ kind: "metadata" });
            setOffset(0);
          }}
        >
          Full metadata · {formatBytes(detail.detail_bytes)}
        </Button>
        {detail.bodies.map((body) => (
          <div className="flex flex-wrap gap-1" key={body.side}>
            <Button
              disabled={!body.available}
              aria-pressed={
                view.kind === "body" && view.side === body.side && !view.decoded
              }
              onClick={() => showBody(body, false)}
            >
              {body.side === "request" ? "Request" : "Response"} ·{" "}
              {formatBytes(body.bytes)}
            </Button>
            {body.content_encoding && (
              <Button
                disabled={!body.decoded_available}
                title={body.decoded_reason ?? "Read decoded content"}
                aria-pressed={
                  view.kind === "body" &&
                  view.side === body.side &&
                  view.decoded
                }
                onClick={() => showBody(body, true)}
              >
                Decode {body.content_encoding}
              </Button>
            )}
          </div>
        ))}
      </div>
      {detail.bodies
        .filter(
          (body) =>
            !body.available ||
            (body.content_encoding && !body.decoded_available),
        )
        .map((body) => (
          <p
            className="rounded-md border border-warning/30 bg-warning/10 px-3 py-2 text-xs leading-relaxed text-warning select-text"
            key={body.side}
          >
            {body.side} · {body.reason ?? body.decoded_reason}
          </p>
        ))}
      {view.kind === "body" && (
        <div className="flex flex-wrap items-center gap-x-3 gap-y-2 px-4 py-2 text-xs">
          <Label className="flex items-center gap-2 text-xs leading-relaxed">
            <Checkbox
              checked={view.binary}
              onCheckedChange={(checked) =>
                setView({ ...view, binary: checked === true })
              }
            />
            Hexadecimal view
          </Label>
          <span className="text-xs leading-relaxed text-muted-foreground">
            {view.decoded
              ? "Decoded content bytes"
              : "Original retained body bytes"}
            . Text uses UTF-8. Chunk boundaries may split a character.
          </span>
        </div>
      )}
      {error && (
        <p
          className="rounded-md border border-warning/30 bg-warning/10 px-3 py-2 text-xs leading-relaxed text-warning select-text"
          role="alert"
        >
          {error}
        </p>
      )}
      <pre
        className={`min-h-40 overflow-auto rounded-md border border-border bg-background p-3 font-mono text-xs leading-5 select-text ${wrap && !(view.kind === "body" && view.binary) ? "whitespace-pre-wrap break-words" : "whitespace-pre"}`}
        tabIndex={0}
      >
        {busy ? "Reading 64 KiB chunk…" : text || "Empty content"}
      </pre>
      <div className="flex shrink-0 flex-wrap items-center justify-between gap-2 border-t border-border px-4 py-2 text-xs tabular-nums [&_button]:h-7 [&_button]:text-xs">
        <Button
          disabled={busy || offset === 0}
          onClick={() => setOffset(Math.max(0, offset - 65536))}
        >
          Previous 64 KiB
        </Button>
        <span>
          {chunk
            ? `Bytes ${chunk.offset.toLocaleString()}–${(chunk.offset + decodeChunk(chunk.data_base64).length).toLocaleString()} of ${chunk.total_bytes.toLocaleString()}`
            : "64 KiB per chunk"}
        </span>
        <Button
          disabled={busy || !chunk || chunk.next_offset === null}
          onClick={() => {
            if (chunk?.next_offset !== null && chunk?.next_offset !== undefined)
              setOffset(chunk.next_offset);
          }}
        >
          Next 64 KiB
        </Button>
      </div>
    </section>
  );
}
