import { useEffect, useRef, useState, type ReactNode } from "react";
import { Button } from "./components/ui/button";
import { BodyWindow } from "./BodyViewport";
import { errorMessage } from "./api";
import { product } from "./product";
import type { DocumentOwner } from "./body-owner";
import type { Match, QueryTarget, queryWindow } from "./body-document";
import type { Window } from "./body-session";

export function QueryParameters({
  owner,
  query,
  active,
  wrap,
}: {
  owner: DocumentOwner;
  query: string;
  active: Match | null;
  wrap: boolean;
}) {
  const [firstRow, setFirstRow] = useState(0);
  const [page, setPage] = useState<ReturnType<typeof queryWindow>>(null);
  const [target, setTarget] = useState<QueryTarget | null>(null);
  const [offset, setOffset] = useState(0);
  const [window, setWindow] = useState<Window | null>(null);
  const [status, setStatus] = useState("");
  useEffect(() => {
    if (active?.region.kind !== "query") return;
    setFirstRow(Math.floor(active.region.row / 50) * 50);
    setTarget({
      kind: "cell",
      row: active.region.row,
      field: active.region.field,
    });
    setOffset(Math.max(0, active.start - 200));
  }, [active]);
  useEffect(() => {
    let disposed = false;
    void owner
      .read({ kind: "query-window", firstRow, query })
      .then((reply) => {
        if (!disposed && reply.kind === "query-window") setPage(reply.window);
      })
      .catch((cause) => {
        if (!disposed) setStatus(errorMessage(cause));
      });
    return () => {
      disposed = true;
    };
  }, [owner, firstRow, query]);
  useEffect(() => {
    let disposed = false;
    setWindow(null);
    if (target)
      void owner
        .read({ kind: "query-text", target, query, offset })
        .then((reply) => {
          if (!disposed && reply.kind === "window") setWindow(reply.window);
        })
        .catch((cause) => {
          if (!disposed) setStatus(errorMessage(cause));
        });
    return () => {
      disposed = true;
    };
  }, [owner, target, query, offset]);
  const live = useRef(true);
  useEffect(() => {
    live.current = true;
    return () => {
      live.current = false;
    };
  }, []);
  async function copy(value: QueryTarget) {
    try {
      const reply = await owner.read({ kind: "query-copy", target: value });
      if (reply.kind !== "copy" || !live.current) return;
      await product.copyText(reply.text);
      if (live.current)
        setStatus(
          value.kind === "raw"
            ? "Copied complete original query."
            : "Copied complete decoded field.",
        );
    } catch (cause) {
      if (live.current) setStatus(`Copy failed. ${errorMessage(cause)}`);
    }
  }
  if (!page)
    return (
      <p role={status ? "alert" : "status"}>
        {status || "Reading query parameters…"}
      </p>
    );
  return (
    <section aria-label="Query parameters" className="space-y-2">
      <h3 className="text-sm font-medium">Query parameters</h3>
      <p className="text-xs text-muted-foreground">
        Decoded query parameters · {page.total.toLocaleString()} rows · + means
        space. Invalid encodings remain literal.
      </p>
      <div className="flex flex-wrap gap-2">
        <Button
          onClick={() => {
            setTarget({ kind: "raw" });
            setOffset(0);
          }}
        >
          Original query
        </Button>
        <Button onClick={() => void copy({ kind: "raw" })}>
          Copy original query
        </Button>
      </div>
      <table
        className="w-full table-fixed text-xs"
        aria-label="Decoded query parameters"
      >
        <thead>
          <tr>
            <th className="text-left">Key</th>
            <th className="text-left">Value</th>
          </tr>
        </thead>
        <tbody>
          {page.rows.map((row, index) => (
            <tr key={page.start + index}>
              {(["key", "value"] as const).map((field) => {
                const cell = row[field];
                const selected =
                  active?.region.kind === "query" &&
                  active.region.row === page.start + index &&
                  active.region.field === field;
                return (
                  <td
                    key={field}
                    className="align-top break-all border-t border-border p-2"
                  >
                    <button
                      className={`text-left select-text ${selected ? "bg-primary text-primary-foreground" : ""}`}
                      onClick={() => {
                        setTarget({
                          kind: "cell",
                          row: page.start + index,
                          field,
                        });
                        setOffset(0);
                      }}
                    >
                      {highlight(cell.text, cell.matches) ||
                        (field === "value" && !row.hasEquals
                          ? "(no equals sign)"
                          : "(empty)")}
                      {cell.total > cell.text.length &&
                        `… (${cell.total.toLocaleString()} characters)`}
                    </button>
                    {cell.decoding === "literal-invalid-encoding" && (
                      <p>
                        Invalid percent or UTF-8 encoding. Literal captured
                        component.
                      </p>
                    )}
                  </td>
                );
              })}
            </tr>
          ))}
        </tbody>
      </table>
      {page.total === 0 && <p>Empty query</p>}
      {page.total > 50 && (
        <nav
          aria-label="Query rows"
          className="flex flex-wrap items-center gap-2 text-xs"
        >
          <Button
            disabled={page.start === 0}
            onClick={() => setFirstRow(Math.max(0, page.start - 50))}
          >
            Previous rows
          </Button>
          <span>
            Rows {page.start + 1}–{page.start + page.rows.length} of{" "}
            {page.total.toLocaleString()}
          </span>
          <Button
            disabled={page.start + page.rows.length === page.total}
            onClick={() => setFirstRow(page.start + page.rows.length)}
          >
            Next rows
          </Button>
        </nav>
      )}
      {target && (
        <div className="space-y-2">
          <p className="text-xs">
            {target.kind === "raw"
              ? `Original query · ${page.rawCharacters.toLocaleString()} characters`
              : `Complete decoded ${target.field} · row ${target.row + 1}`}
          </p>
          <Button onClick={() => void copy(target)}>
            Copy complete{" "}
            {target.kind === "raw" ? "original query" : target.field}
          </Button>
          {window && (
            <BodyWindow
              window={window}
              onOffset={setOffset}
              wrap={wrap}
              active={
                target.kind === "cell" &&
                active?.region.kind === "query" &&
                active.region.row === target.row &&
                active.region.field === target.field
                  ? active
                  : null
              }
              label={
                target.kind === "raw" ? "Original query" : "Query field content"
              }
            />
          )}
        </div>
      )}
      {status && <p role="status">{status}</p>}
    </section>
  );
}

function highlight(
  text: string,
  matches: { start: number; end: number }[],
): ReactNode {
  if (!text || !matches.length) return text;
  const runs: ReactNode[] = [];
  let offset = 0;
  for (const match of matches) {
    runs.push(text.slice(offset, match.start));
    runs.push(
      <mark key={match.start}>{text.slice(match.start, match.end)}</mark>,
    );
    offset = match.end;
  }
  runs.push(text.slice(offset));
  return runs;
}
