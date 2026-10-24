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
  | { kind: "idle" }
  | { kind: "pending" }
  | { kind: "error"; message: string };
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
      className="details"
      aria-label="Event details"
      aria-busy={busy}
    >
      <div className="section-heading">
        {offPage && <span className="badge">Selected outside this page</span>}
        <button
          className="text"
          disabled={reveal.kind === "pending"}
          onClick={() => void revealRaw()}
        >
          {reveal.kind === "pending"
            ? "Revealing raw source…"
            : "Reveal raw source"}
        </button>
      </div>
      {reveal.kind === "error" && (
        <p className="notice" role="alert">{reveal.message}</p>
      )}
      <dl className="event-facts">
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
      <p className="detail-summary">{detail.event.summary}</p>
      <div className="detail-tabs">
        <button
          aria-pressed={view.kind === "metadata"}
          onClick={() => {
            setView({ kind: "metadata" });
            setOffset(0);
          }}
        >
          Full metadata · {formatBytes(detail.detail_bytes)}
        </button>
        {detail.bodies.map((body) => (
          <div className="body-choice" key={body.side}>
            <button
              disabled={!body.available}
              aria-pressed={
                view.kind === "body" &&
                view.side === body.side &&
                !view.decoded
              }
              onClick={() => showBody(body, false)}
            >
              {body.side === "request" ? "Request" : "Response"} ·{" "}
              {formatBytes(body.bytes)}
            </button>
            {body.content_encoding && (
              <button
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
              </button>
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
          <p className="notice" key={body.side}>
            {body.side} · {body.reason ?? body.decoded_reason}
          </p>
        ))}
      {view.kind === "body" && (
        <div className="reading-controls">
          <label className="inline">
            <input
              type="checkbox"
              checked={view.binary}
              onChange={(event) =>
                setView({ ...view, binary: event.target.checked })
              }
            />
            Hexadecimal view
          </label>
          <span className="muted">
            {view.decoded
              ? "Decoded content bytes"
              : "Original retained body bytes"}
            . Text uses UTF-8. Chunk boundaries may split a character.
          </span>
        </div>
      )}
      {error && (
        <p className="notice" role="alert">
          {error}
        </p>
      )}
      <pre
        className={`chunk ${wrap && !(view.kind === "body" && view.binary) ? "wrap" : ""}`}
        tabIndex={0}
      >
        {busy ? "Reading 64 KiB chunk…" : text || "Empty content"}
      </pre>
      <div className="pagination">
        <button
          disabled={busy || offset === 0}
          onClick={() => setOffset(Math.max(0, offset - 65536))}
        >
          Previous 64 KiB
        </button>
        <span>
          {chunk
            ? `Bytes ${chunk.offset.toLocaleString()}–${(chunk.offset + decodeChunk(chunk.data_base64).length).toLocaleString()} of ${chunk.total_bytes.toLocaleString()}`
            : "64 KiB per chunk"}
        </span>
        <button
          disabled={busy || !chunk || chunk.next_offset === null}
          onClick={() => {
            if (
              chunk?.next_offset !== null &&
              chunk?.next_offset !== undefined
            )
              setOffset(chunk.next_offset);
          }}
        >
          Next 64 KiB
        </button>
      </div>
    </section>
  );
}
