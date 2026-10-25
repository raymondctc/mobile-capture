import { useEffect, useRef, useState } from "react";
import { Button } from "./components/ui/button";
import {
  decodeChunk,
  product,
  type BodySide,
  type DataChunk,
  type EventRef,
} from "./product";
import { errorMessage } from "./api";

type State =
  | { kind: "loading" }
  | { kind: "ready"; chunk: DataChunk }
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
export function DetailChunks({
  reference,
  side,
  wrap,
}: {
  reference: EventRef;
  side?: BodySide;
  wrap: boolean;
}) {
  const [representation, setRepresentation] = useState<"hex" | "base64">("hex");
  const [action, setAction] = useState<
    { kind: "idle" } | { kind: "pending" } | { kind: "message"; text: string }
  >({ kind: "idle" });
  const revision = useRef(0);
  const [offset, setOffset] = useState(0);
  const [state, setState] = useState<State>({ kind: "loading" });
  useEffect(() => {
    setAction({ kind: "idle" });
    setOffset(0);
    setRepresentation("hex");
    return () => {
      revision.current += 1;
    };
  }, [reference.session_key, reference.event_id, side]);
  const pageSize = side && representation === "base64" ? 65535 : 65536;
  useEffect(() => {
    let disposed = false;
    setState({ kind: "loading" });
    const load = side
      ? product.body(reference, side, offset, pageSize).then((result) => {
          if (!result.body.available)
            throw new Error(
              result.body.reason ?? "Retained body bytes unavailable",
            );
          return result.chunk;
        })
      : product.detailChunk(reference, offset);
    void load
      .then((chunk) => {
        if (!disposed) setState({ kind: "ready", chunk });
      })
      .catch((cause: unknown) => {
        if (!disposed)
          setState({ kind: "error", message: errorMessage(cause) });
      });
    return () => {
      disposed = true;
    };
  }, [reference.session_key, reference.event_id, side, offset, pageSize]);
  const chunk = state.kind === "ready" ? state.chunk : null;
  const bytes = chunk ? decodeChunk(chunk.data_base64) : new Uint8Array();
  const text = side
    ? representation === "hex"
      ? hexChunk(bytes, offset)
      : (chunk?.data_base64 ?? "")
    : new TextDecoder().decode(bytes);
  async function retainedAction(kind: "copy-base64" | "save") {
    if (!side) return;
    const request = revision.current;
    setAction({ kind: "pending" });
    try {
      const result = await product.retainedBody(reference, side, kind);
      if (request !== revision.current) return;
      setAction({
        kind: "message",
        text:
          result.kind === "cancelled"
            ? "Save cancelled."
            : result.kind === "copied"
              ? `Copied complete Base64 · ${result.bytes.toLocaleString()} retained bytes.`
              : `Saved original body · ${result.bytes.toLocaleString()} retained bytes.`,
      });
    } catch (cause) {
      if (request === revision.current)
        setAction({
          kind: "message",
          text: `${kind === "save" ? "Save" : "Copy"} failed. ${errorMessage(cause)}`,
        });
    }
  }
  return (
    <div className="space-y-3">
      <p className="text-xs text-muted-foreground">
        {side
          ? "Original retained body bytes, before content decoding."
          : "Full metadata and headers. UTF-8 chunk boundaries may split a character."}
      </p>
      {side && (
        <div className="flex flex-wrap gap-2">
          <Button
            aria-pressed={representation === "hex"}
            onClick={() => {
              setRepresentation("hex");
              setOffset(0);
            }}
          >
            Hex
          </Button>
          <Button
            aria-pressed={representation === "base64"}
            onClick={() => {
              setRepresentation("base64");
              setOffset(0);
            }}
          >
            Base64
          </Button>
          <Button
            disabled={action.kind === "pending"}
            onClick={() => void retainedAction("copy-base64")}
          >
            Copy Base64
          </Button>
          <Button
            disabled={action.kind === "pending"}
            onClick={() => void retainedAction("save")}
          >
            Save original body
          </Button>
        </div>
      )}
      {action.kind === "pending" && (
        <p role="status">Preparing retained body…</p>
      )}
      {action.kind === "message" && <p role="status">{action.text}</p>}
      {state.kind === "error" && <p role="alert">{state.message}</p>}
      <pre
        tabIndex={0}
        className={`min-h-32 overflow-auto rounded-md border border-border p-3 font-mono text-xs select-text ${wrap && !side ? "whitespace-pre-wrap break-words" : "whitespace-pre"}`}
      >
        {state.kind === "loading"
          ? "Reading byte page…"
          : text || "Empty content"}
      </pre>
      <nav
        aria-label={side ? "Retained byte chunks" : "Metadata chunks"}
        className="flex flex-wrap items-center justify-between gap-2 text-xs tabular-nums"
      >
        <Button
          disabled={state.kind === "loading" || offset === 0}
          onClick={() => setOffset(Math.max(0, offset - pageSize))}
        >
          Previous{" "}
          {representation === "base64" && side ? "65,535 bytes" : "64 KiB"}
        </Button>
        <span>
          {chunk
            ? `Bytes ${chunk.offset.toLocaleString()}–${(chunk.offset + bytes.length).toLocaleString()} of ${chunk.total_bytes.toLocaleString()}`
            : `${pageSize.toLocaleString()} bytes per page`}
        </span>
        <Button
          disabled={!chunk || chunk.next_offset === null}
          onClick={() => {
            if (chunk?.next_offset !== null && chunk?.next_offset !== undefined)
              setOffset(chunk.next_offset);
          }}
        >
          Next {representation === "base64" && side ? "65,535 bytes" : "64 KiB"}
        </Button>
      </nav>
    </div>
  );
}
