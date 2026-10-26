import { useEffect, useState } from "react";
import { Button } from "./components/ui/button";
import { BodyWindow, highlightText } from "./BodyViewport";
import { errorMessage } from "./api";
import type { DocumentOwner } from "./body-owner";
import type { HeaderTarget, Match, headerWindow } from "./body-document";
import type { Window } from "./body-session";
import type { BodySide } from "./product";

export function Headers({
  owner,
  side,
  query,
  active,
  wrap,
}: {
  owner: DocumentOwner;
  side: BodySide;
  query: string;
  active: Match | null;
  wrap: boolean;
}) {
  const [firstRow, setFirstRow] = useState(0);
  const [page, setPage] = useState<ReturnType<typeof headerWindow>>(null);
  const [target, setTarget] = useState<HeaderTarget | null>(null);
  const [offset, setOffset] = useState(0);
  const [window, setWindow] = useState<Window | null>(null);
  const [error, setError] = useState("");
  useEffect(() => {
    if (active?.region.kind !== "header" || active.region.side !== side) return;
    setFirstRow(Math.floor(active.region.row / 50) * 50);
    setTarget(active.region);
    setOffset(Math.max(0, active.start - 200));
  }, [active, side]);
  useEffect(() => {
    let disposed = false;
    void owner
      .read({ kind: "header-window", side, firstRow, query })
      .then((reply) => {
        if (!disposed && reply.kind === "header-window") setPage(reply.window);
      })
      .catch((cause: unknown) => {
        if (!disposed) setError(errorMessage(cause));
      });
    return () => {
      disposed = true;
    };
  }, [owner, side, firstRow, query]);
  useEffect(() => {
    let disposed = false;
    setWindow(null);
    if (target)
      void owner
        .read({ kind: "header-text", target, offset, query })
        .then((reply) => {
          if (!disposed && reply.kind === "window") setWindow(reply.window);
        })
        .catch((cause: unknown) => {
          if (!disposed) setError(errorMessage(cause));
        });
    return () => {
      disposed = true;
    };
  }, [owner, target, offset, query]);
  if (error) return <p role="alert">{error}</p>;
  if (!page) return <p role="status">Reading headers…</p>;
  const source = side === "request" ? "Request" : "Response";
  return (
    <div className="space-y-2">
      <table
        className="w-full table-fixed text-xs"
        aria-label={`${source} header fields`}
      >
        <thead>
          <tr>
            <th className="text-left">Name</th>
            <th className="text-left">Value</th>
          </tr>
        </thead>
        <tbody>
          {page.rows.map((row, index) => (
            <tr key={page.start + index}>
              {(["name", "value"] as const).map((field) => {
                const cell = row[field];
                const selected =
                  active?.region.kind === "header" &&
                  active.region.side === side &&
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
                        setTarget({ side, row: page.start + index, field });
                        setOffset(0);
                      }}
                    >
                      {highlightText(cell.text, cell.matches) || "(empty)"}
                      {cell.total > cell.text.length
                        ? `… (${cell.total.toLocaleString()} characters)`
                        : ""}
                    </button>
                  </td>
                );
              })}
            </tr>
          ))}
        </tbody>
      </table>
      {page.total === 0 && <p>Recorded empty header list.</p>}
      {page.total > 50 && (
        <nav
          aria-label={`${source} header rows`}
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
      {target && window && (
        <div className="space-y-2">
          <p className="text-xs">
            Complete {source.toLowerCase()} header {target.field} · row{" "}
            {target.row + 1}
          </p>
          <BodyWindow
            window={window}
            onOffset={setOffset}
            wrap={wrap}
            label={`${source} header ${target.field}`}
            emptyLabel="Empty header field"
            active={
              active?.region.kind === "header" &&
              active.region.side === side &&
              active.region.row === target.row &&
              active.region.field === target.field
                ? active
                : null
            }
          />
        </div>
      )}
    </div>
  );
}
