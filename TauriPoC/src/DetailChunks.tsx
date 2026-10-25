import { useEffect, useState } from "react";
import { Button } from "./components/ui/button";
import { decodeChunk, product, type BodySide, type DataChunk } from "./product";
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
  id,
  side,
  wrap,
}: {
  id: string;
  side?: BodySide;
  wrap: boolean;
}) {
  const [offset, setOffset] = useState(0);
  const [state, setState] = useState<State>({ kind: "loading" });
  useEffect(() => {
    let disposed = false;
    setState({ kind: "loading" });
    const load = side
      ? product.body(id, side, offset).then((result) => {
          if (!result.body.available)
            throw new Error(
              result.body.reason ?? "Retained body bytes unavailable",
            );
          return result.chunk;
        })
      : product.detailChunk(id, offset);
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
  }, [id, side, offset]);
  const chunk = state.kind === "ready" ? state.chunk : null;
  const bytes = chunk ? decodeChunk(chunk.data_base64) : new Uint8Array();
  const text = side ? hexChunk(bytes, offset) : new TextDecoder().decode(bytes);
  return (
    <div className="space-y-3">
      <p className="text-xs text-muted-foreground">
        {side
          ? "Original retained body bytes, before content decoding."
          : "Full metadata and headers. UTF-8 chunk boundaries may split a character."}
      </p>
      {state.kind === "error" && <p role="alert">{state.message}</p>}
      <pre
        tabIndex={0}
        className={`min-h-32 overflow-auto rounded-md border border-border p-3 font-mono text-xs select-text ${wrap && !side ? "whitespace-pre-wrap break-words" : "whitespace-pre"}`}
      >
        {state.kind === "loading"
          ? "Reading 64 KiB chunk…"
          : text || "Empty content"}
      </pre>
      <nav
        aria-label={side ? "Retained byte chunks" : "Metadata chunks"}
        className="flex flex-wrap items-center justify-between gap-2 text-xs tabular-nums"
      >
        <Button
          disabled={state.kind === "loading" || offset === 0}
          onClick={() => setOffset(Math.max(0, offset - 65536))}
        >
          Previous 64 KiB
        </Button>
        <span>
          {chunk
            ? `Bytes ${chunk.offset.toLocaleString()}–${(chunk.offset + bytes.length).toLocaleString()} of ${chunk.total_bytes.toLocaleString()}`
            : "64 KiB per chunk"}
        </span>
        <Button
          disabled={!chunk || chunk.next_offset === null}
          onClick={() => {
            if (chunk?.next_offset !== null && chunk?.next_offset !== undefined)
              setOffset(chunk.next_offset);
          }}
        >
          Next 64 KiB
        </Button>
      </nav>
    </div>
  );
}
