import { Progress } from "./components/ui/progress";
import { Badge } from "./components/ui/badge";
import { Checkbox } from "./components/ui/checkbox";
import { Button } from "./components/ui/button";
import { Input } from "./components/ui/input";
import { Label } from "./components/ui/label";
import { NativeSelect } from "./components/ui/native-select";
import { useEffect, useRef, useState } from "react";
import { errorMessage } from "./api";
import {
  formatBytes,
  product,
  type EventDetail,
  type SessionDescriptor,
  type TimelinePage,
  type TimelineQuery,
} from "./product";
import { timelineFence, useReading } from "./state";
import { Details } from "./Details";
import { X } from "lucide-react";

type QueryState = {
  source: string;
  search: string;
  cursor: string | null;
  previous: (string | null)[];
  after: number | null;
  through: number | null;
  revision: number;
};
type InspectorEvent = {
  directory: SessionDescriptor["directory"];
  eventId: EventDetail["event"]["id"];
};
type Inspector =
  | { kind: "closed" }
  | (InspectorEvent &
      (
        | { kind: "loading" }
        | { kind: "ready"; detail: EventDetail }
        | { kind: "error"; message: string }
      ));
const initialQuery: QueryState = {
  source: "",
  search: "",
  cursor: null,
  previous: [],
  after: null,
  through: null,
  revision: 0,
};
export function Timeline({
  inspected,
}: {
  inspected: SessionDescriptor | undefined;
}) {
  const [reading, setReading] = useReading();
  const [query, setQuery] = useState<QueryState>(initialQuery);
  const [page, setPage] = useState<TimelinePage>();
  const [inspector, setInspector] = useState<Inspector>({ kind: "closed" });
  const [error, setError] = useState("");
  const [invalidation, setInvalidation] = useState("");
  const [loading, setLoading] = useState(false);
  const generation = useRef(0);
  const detailGeneration = useRef(0);
  const returnFocus = useRef<HTMLButtonElement | null>(null);
  const closeButton = useRef<HTMLButtonElement>(null);
  const heading = useRef<HTMLHeadingElement>(null);
  const directory = useRef(inspected?.directory);
  const list = useRef<HTMLDivElement>(null);
  const follow = useRef(reading.follow);
  const paused = query.through !== null;
  follow.current = reading.follow;
  directory.current = inspected?.directory;
  const visibleInspector =
    inspector.kind !== "closed" && inspector.directory === inspected?.directory
      ? inspector
      : undefined;
  useEffect(() => {
    setQuery(initialQuery);
    setPage(undefined);
    setInspector({ kind: "closed" });
    returnFocus.current = null;
    setInvalidation("");
    detailGeneration.current += 1;
    return () => {
      detailGeneration.current += 1;
    };
  }, [inspected?.directory]);
  useEffect(() => {
    if (visibleInspector) closeButton.current?.focus();
  }, [visibleInspector?.directory, visibleInspector?.eventId]);
  useEffect(() => {
    const request = ++generation.current;
    let disposed = false;
    let timer: ReturnType<typeof setTimeout>;
    if (!inspected) return;
    const input: TimelineQuery = {
      source: query.source || null,
      search: query.search,
      order: reading.order,
      limit: 200,
      cursor: query.cursor,
      after_sequence: query.after,
      through_sequence: query.through,
    };
    setLoading(true);
    setError("");
    async function poll() {
      try {
        const result = await product.query(input);
        if (disposed || request !== generation.current) return;
        setPage(result);
        setError("");
        setLoading(false);
        if (
          follow.current &&
          query.cursor === null &&
          query.through === null &&
          list.current
        )
          list.current.scrollTop =
            reading.order === "newest_first" ? 0 : list.current.scrollHeight;
      } catch (cause) {
        if (disposed || request !== generation.current) return;
        setError(errorMessage(cause));
        setLoading(false);
        if (input.cursor) {
          setInvalidation(
            `The previous page could not be read. ${errorMessage(cause)} The first page has reloaded with your filters preserved.`,
          );
          setQuery((previous) => ({
            ...previous,
            cursor: null,
            previous: [],
            revision: previous.revision + 1,
          }));
        }
      }
      if (!disposed) timer = setTimeout(poll, 1000);
    }
    void poll();
    return () => {
      disposed = true;
      clearTimeout(timer);
    };
  }, [inspected?.directory, query, reading.order]);
  async function select(eventId: string, origin: HTMLButtonElement) {
    if (!inspected) return;
    const selected = { directory: inspected.directory, eventId };
    const request = ++detailGeneration.current;
    returnFocus.current = origin;
    setInspector({ kind: "loading", ...selected });
    try {
      const detail = await product.detail(eventId);
      if (
        request !== detailGeneration.current ||
        directory.current !== selected.directory
      )
        return;
      setInspector(
        detail
          ? { kind: "ready", ...selected, detail }
          : {
              kind: "error",
              ...selected,
              message:
                "The selected event is no longer available. Refresh the session.",
            },
      );
    } catch (cause) {
      if (
        request === detailGeneration.current &&
        directory.current === selected.directory
      )
        setInspector({
          kind: "error",
          ...selected,
          message: errorMessage(cause),
        });
    }
  }
  function closeInspector() {
    detailGeneration.current += 1;
    setInspector({ kind: "closed" });
    const target = returnFocus.current;
    if (target?.isConnected) target.focus();
    else heading.current?.focus();
    returnFocus.current = null;
  }
  function reset(changes: Partial<QueryState>) {
    setQuery((previous) => ({
      ...previous,
      ...changes,
      cursor: null,
      previous: [],
    }));
  }
  function fence(action: "pause" | "clear" | "full") {
    const next = timelineFence(
      action,
      page?.progress.latest_sequence ?? 0,
      query,
    );
    reset(next);
    if (action !== "pause")
      setReading({ ...reading, follow: reading.order === "newest_first" });
  }
  function latest() {
    reset({ through: null });
    setReading({ ...reading, follow: true, order: "newest_first" });
    if (list.current)
      list.current.scrollTop =
        reading.order === "newest_first" ? 0 : list.current.scrollHeight;
  }
  const selectionOffPage =
    visibleInspector &&
    !page?.rows.some((row) => row.event.id === visibleInspector.eventId);
  return (
    <section
      className="relative flex h-[calc(100dvh-18rem)] min-h-[32rem] shrink-0 overflow-hidden sm:min-h-[24rem]"
      aria-label="Captured evidence"
    >
      <div className="flex min-h-0 min-w-0 flex-1 flex-col">
        <div className="flex shrink-0 flex-wrap items-center justify-between gap-2 px-4 pt-2 [&>div:first-child]:flex [&>div:first-child]:flex-wrap [&>div:first-child]:items-baseline [&>div:first-child]:gap-3 [&_h2]:text-base [&_h2]:font-medium">
          <div>
            <h2 ref={heading} tabIndex={-1}>
              Timeline
            </h2>
            <p className="text-xs leading-relaxed text-muted-foreground">
              {inspected
                ? `${page?.matching.toLocaleString() ?? "…"} matching · ${page?.total.toLocaleString() ?? "…"} indexed events`
                : "Open a saved session or start a recording."}
            </p>
          </div>
          <div className="flex flex-wrap items-center gap-2">
            <Button
              disabled={!inspected}
              onClick={() =>
                paused ? reset({ through: null }) : fence("pause")
              }
            >
              {paused ? "Resume" : "Pause view"}
            </Button>
            <Button disabled={!inspected} onClick={latest}>
              Jump to latest
            </Button>
          </div>
        </div>
        <div className="flex shrink-0 flex-wrap gap-2 px-4 pt-2 [&>input]:min-w-36 [&>input]:flex-1">
          <Input
            type="search"
            aria-label="Search the complete session"
            placeholder="Search full events and body text"
            value={query.search}
            onChange={(event) => reset({ search: event.target.value })}
            disabled={!inspected}
          />
          <NativeSelect
            aria-label="Source filter"
            value={query.source}
            onChange={(event) => reset({ source: event.target.value })}
            disabled={!inspected}
          >
            <option value="">All sources</option>
            <option value="network">Network</option>
            <option value="logs">Logs</option>
            <option value="marker">Markers</option>
          </NativeSelect>
          <NativeSelect
            aria-label="Timeline order"
            value={reading.order}
            onChange={(event) => {
              const value = event.target.value;
              if (value === "oldest_first" || value === "newest_first") {
                reset({});
                setReading({
                  ...reading,
                  order: value,
                  follow: value === "newest_first" && reading.follow,
                });
              }
            }}
          >
            <option value="newest_first">Newest first</option>
            <option value="oldest_first">Oldest first</option>
          </NativeSelect>
        </div>
        <div className="flex flex-wrap items-center gap-x-3 gap-y-2 px-4 py-2 text-xs">
          <Label className="flex items-center gap-2 text-xs leading-relaxed">
            <Checkbox
              checked={reading.follow}
              onCheckedChange={(checked) => {
                if (checked === true) reset({ through: null });
                setReading({
                  ...reading,
                  follow: checked === true,
                  order: checked === true ? "newest_first" : reading.order,
                });
              }}
            />
            Follow latest
          </Label>
          <Label className="flex items-center gap-2 text-xs leading-relaxed">
            <Checkbox
              checked={reading.wrap}
              onCheckedChange={(checked) =>
                setReading({ ...reading, wrap: checked === true })
              }
            />
            Wrap lines
          </Label>
          <Button
            variant="ghost"
            disabled={!inspected}
            onClick={() => fence("clear")}
          >
            Clear view
          </Button>
          <Button
            variant="ghost"
            disabled={!inspected}
            onClick={() => fence("full")}
          >
            Full session
          </Button>
          <span className="text-xs leading-relaxed text-muted-foreground">
            {paused
              ? `Paused through event ${query.through}. Capture continues.`
              : query.after !== null
                ? `Showing events after ${query.after}. Evidence remains saved.`
                : "Display controls do not change evidence."}
          </span>
        </div>
        {invalidation && (
          <div
            className="rounded-md border border-warning/30 bg-warning/10 px-3 py-2 text-xs leading-relaxed text-warning select-text"
            role="status"
          >
            {invalidation}{" "}
            <Button variant="ghost" onClick={() => setInvalidation("")}>
              Dismiss
            </Button>
          </div>
        )}
        {error && (
          <div
            className="rounded-md border border-warning/30 bg-warning/10 px-3 py-2 text-xs leading-relaxed text-warning select-text"
            role="alert"
          >
            {error}
            {query.cursor === null && (
              <span>
                {" "}
                The first page will reload. Your filters are preserved.
              </span>
            )}
          </div>
        )}
        {page && !page.progress.complete && (
          <div
            className="flex shrink-0 items-center gap-3 px-4 pb-1 text-xs text-muted-foreground"
            role="status"
          >
            <span>
              {page.progress.complete
                ? "Index up to date"
                : `Indexing · ${formatBytes(page.progress.indexed_bytes)} of ${formatBytes(page.progress.raw_bytes)} · ${formatBytes(page.progress.backlog_bytes)} pending`}
            </span>
            {!page.progress.complete && (
              <Progress
                className="h-1.5 w-32"
                value={
                  (page.progress.indexed_bytes /
                    Math.max(page.progress.raw_bytes, 1)) *
                  100
                }
                aria-label="Indexing evidence"
              />
            )}
          </div>
        )}
        <div
          className={`min-h-0 flex-1 overflow-auto overscroll-contain ${reading.wrap ? "[&_.event-summary]:whitespace-pre-wrap [&_.event-summary]:break-words" : "[&_.event-summary]:whitespace-pre"}`}
          ref={list}
          aria-busy={loading}
          onScroll={() => {
            const element = list.current;
            if (!element || !reading.follow) return;
            const atEdge =
              reading.order === "newest_first"
                ? element.scrollTop <= 4
                : element.scrollHeight -
                    element.clientHeight -
                    element.scrollTop <=
                  4;
            if (!atEdge) setReading({ ...reading, follow: false });
          }}
          onWheel={() => {
            if (reading.follow) setReading({ ...reading, follow: false });
          }}
        >
          {!page?.rows.length ? (
            <div className="mx-auto flex min-h-48 max-w-md flex-col justify-center gap-2 px-6 py-6 text-center [&_h3]:font-medium [&_p]:text-sm [&_p]:leading-relaxed [&_p]:text-muted-foreground">
              <h3>
                {loading
                  ? "Reading evidence…"
                  : inspected
                    ? "No matching events"
                    : "Your evidence appears here"}
              </h3>
              <p>
                {inspected
                  ? "Change the filters, resume the view, or show the full session. An indexing backlog may take time to finish."
                  : "Choose an Android device and record, or open a session folder. Saved evidence can be read without capture tools."}
              </p>
            </div>
          ) : (
            <div role="list" aria-label="Timeline events">
              {page.rows.map(({ sequence, event }) => (
                <div role="listitem" key={event.id}>
                  <Button
                    aria-pressed={visibleInspector?.eventId === event.id}
                    aria-haspopup="dialog"
                    variant="ghost"
                    className={`grid h-auto min-h-16 w-full grid-cols-[70px_48px_minmax(0,1fr)] items-start justify-start gap-3 rounded-none border-0 border-b border-border px-4 py-3 text-left font-normal whitespace-normal select-text ${visibleInspector?.eventId === event.id ? "bg-accent hover:bg-accent" : "hover:bg-muted/50"}`}
                    onClick={(click) =>
                      void select(event.id, click.currentTarget)
                    }
                  >
                    <span
                      className="flex shrink-0 flex-col gap-1 text-xs tabular-nums text-muted-foreground [&_small]:text-xs"
                      title={event.timestamp}
                    >
                      {new Date(event.timestamp).toLocaleTimeString()}
                      <small>#{sequence}</small>
                    </span>
                    <Badge
                      variant="secondary"
                      className={`mt-0.5 h-5 px-1.5 text-xs ${event.source === "network" ? "bg-accent text-accent-foreground" : ""}`}
                    >
                      {event.source === "network"
                        ? "NET"
                        : event.source === "marker"
                          ? "MARK"
                          : "LOG"}
                    </Badge>
                    <span className="flex min-w-0 flex-col gap-1 [&_strong]:text-xs [&_strong]:font-normal [&_strong]:text-muted-foreground [&>span]:text-sm [&>span]:leading-5">
                      <strong>{event.kind.replaceAll("_", " ")}</strong>
                      <span className="event-summary select-text">
                        {event.summary}
                      </span>
                    </span>
                  </Button>
                </div>
              ))}
            </div>
          )}
        </div>
        <div className="flex shrink-0 flex-wrap items-center justify-between gap-2 border-t border-border px-4 py-2 text-xs tabular-nums [&_button]:h-7 [&_button]:text-xs">
          <Button
            disabled={!query.previous.length || loading}
            onClick={() => {
              const history = query.previous.slice(0, -1);
              const cursor = query.previous.at(-1) ?? null;
              setQuery({ ...query, cursor, previous: history });
              setReading({ ...reading, follow: false });
            }}
          >
            {reading.order === "newest_first" ? "Newer page" : "Earlier page"}
          </Button>
          <span>
            {page?.rows.length ?? 0} events on this page · page{" "}
            {query.previous.length + 1}
          </span>
          <Button
            disabled={!page?.next_cursor || loading}
            onClick={() => {
              if (page?.next_cursor) {
                setQuery({
                  ...query,
                  cursor: page.next_cursor,
                  previous: [...query.previous, query.cursor],
                });
                setReading({ ...reading, follow: false });
              }
            }}
          >
            {reading.order === "newest_first" ? "Older page" : "Later page"}
          </Button>
        </div>
      </div>
      {visibleInspector && (
        <aside
          className="absolute inset-0 z-10 flex min-h-0 min-w-0 flex-col border-l border-border bg-card md:static md:w-[clamp(320px,34vw,400px)] md:shrink-0"
          role="dialog"
          aria-modal={false}
          aria-labelledby="event-inspector-title"
          onKeyDown={(event) => {
            if (event.key === "Escape") {
              event.preventDefault();
              event.stopPropagation();
              closeInspector();
            }
          }}
        >
          <div className="flex shrink-0 flex-wrap items-center justify-between gap-2 border-b border-border px-4 py-3 [&_h2]:text-base [&_h2]:font-medium [&_button]:h-7 [&_button]:text-xs">
            <h2 id="event-inspector-title">Event details</h2>
            <Button
              ref={closeButton}
              size="icon-sm"
              aria-label="Close event details"
              title="Close event details"
              onClick={closeInspector}
            >
              <X aria-hidden="true" />
            </Button>
          </div>
          <div className="min-h-0 flex-1 overflow-y-auto overscroll-contain px-4 py-4">
            {visibleInspector.kind === "loading" && (
              <p role="status">Reading event…</p>
            )}
            {visibleInspector.kind === "error" && (
              <p
                className="rounded-md border border-warning/30 bg-warning/10 px-3 py-2 text-xs leading-relaxed text-warning select-text"
                role="alert"
              >
                {visibleInspector.message}
              </p>
            )}
            {visibleInspector.kind === "ready" && (
              <Details
                key={JSON.stringify([
                  visibleInspector.directory,
                  visibleInspector.eventId,
                ])}
                detail={visibleInspector.detail}
                offPage={!!selectionOffPage}
                wrap={reading.wrap}
              />
            )}
          </div>
        </aside>
      )}
    </section>
  );
}
