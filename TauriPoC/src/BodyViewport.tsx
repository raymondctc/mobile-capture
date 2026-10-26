import { useEffect, useRef, type ReactNode } from "react";
import { Button } from "./components/ui/button";
import { bodyWindow, type Match } from "./body-document";
import type { Window } from "./body-session";

export function highlightText(
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
  emptyLabel = "Empty body",
}: {
  window: Window;
  onOffset: (offset: number) => void;
  wrap: boolean;
  active?: Match | null;
  label?: string;
  emptyLabel?: string;
}) {
  const activeMark = useRef<HTMLElement>(null);
  useEffect(() => {
    activeMark.current?.scrollIntoView({ block: "nearest" });
  }, [active, window]);
  const runs: ReactNode[] = [];
  let position = window.start;
  const slice = (start: number, end: number) =>
    window.text.slice(start - window.start, end - window.start);
  for (const match of window.matches) {
    const selected =
      active && active.start < match.end && active.end > match.start;
    if (match.start > position) runs.push(slice(position, match.start));
    runs.push(
      <mark
        key={match.start}
        ref={selected ? activeMark : null}
        className={
          selected
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
        {window.total ? runs : emptyLabel}
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
