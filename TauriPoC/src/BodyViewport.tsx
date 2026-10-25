import type { ReactNode } from "react";
import { Button } from "./components/ui/button";
import { bodyWindow, type Match } from "./body-document";
import type { Window } from "./body-session";

export function BodyViewport({
  text,
  offset,
  onOffset,
  wrap,
}: {
  text: string;
  offset: number;
  onOffset: (offset: number) => void;
  wrap: boolean;
}) {
  return (
    <BodyWindow
      window={{ ...bodyWindow(text, offset), matches: [] }}
      onOffset={onOffset}
      wrap={wrap}
    />
  );
}
export function BodyWindow({
  window,
  onOffset,
  wrap,
  active = null,
  label = "Body content",
}: {
  window: Window;
  onOffset: (offset: number) => void;
  wrap: boolean;
  active?: Match | null;
  label?: string;
}) {
  const runs: ReactNode[] = [];
  let position = window.start;
  const slice = (start: number, end: number) =>
    window.text.slice(start - window.start, end - window.start);
  for (const match of window.matches) {
    if (match.start > position) runs.push(slice(position, match.start));
    runs.push(
      <mark
        key={match.start}
        className={
          active && active.start < match.end && active.end > match.start
            ? "bg-primary text-primary-foreground"
            : "bg-amber-200 text-black dark:bg-amber-700 dark:text-white"
        }
      >
        {slice(match.start, match.end)}
      </mark>,
    );
    position = match.end;
  }
  if (position < window.end) runs.push(slice(position, window.end));
  return (
    <div className="space-y-2">
      <pre
        aria-label={label}
        tabIndex={0}
        className={`min-h-32 overflow-auto rounded-md border border-border bg-background p-3 font-mono text-xs leading-5 select-text ${wrap ? "whitespace-pre-wrap break-words" : "whitespace-pre"}`}
      >
        {window.total ? runs : "Empty body"}
      </pre>
      {(window.start > 0 || window.end < window.total) && (
        <nav
          aria-label="Body window"
          className="flex flex-wrap items-center justify-between gap-2 text-xs tabular-nums"
        >
          <Button
            disabled={window.start === 0}
            onClick={() => onOffset(window.previous)}
          >
            Previous window
          </Button>
          <span>
            {window.total
              ? `Characters ${(window.start + 1).toLocaleString()}–${window.end.toLocaleString()} of ${window.total.toLocaleString()}`
              : "0 characters"}
          </span>
          <Button
            disabled={window.end === window.total}
            onClick={() => onOffset(window.end)}
          >
            Next window
          </Button>
        </nav>
      )}
    </div>
  );
}
