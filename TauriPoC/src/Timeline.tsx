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
        setInspector({ kind: "error", ...selected, message: errorMessage(cause) });
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
    <section className="timeline-workspace" aria-label="Captured evidence">
      <div className="timeline-heading">
        <div>
          <h2 ref={heading} tabIndex={-1}>
            Timeline
          </h2>
          <p className="muted">
            {inspected
              ? `${page?.matching.toLocaleString() ?? "…"} matching · ${page?.total.toLocaleString() ?? "…"} indexed events`
              : "Open a saved session or start a recording."}
          </p>
        </div>
        <div className="button-row">
          <button
            disabled={!inspected}
            onClick={() => (paused ? reset({ through: null }) : fence("pause"))}
          >
            {paused ? "Resume" : "Pause view"}
          </button>
          <button disabled={!inspected} onClick={latest}>
            Jump to latest
          </button>
        </div>
      </div>
      <div className="timeline-toolbar">
        <input
          type="search"
          aria-label="Search the complete session"
          placeholder="Search full events and body text"
          value={query.search}
          onChange={(event) => reset({ search: event.target.value })}
          disabled={!inspected}
        />
        <select
          aria-label="Source filter"
          value={query.source}
          onChange={(event) => reset({ source: event.target.value })}
          disabled={!inspected}
        >
          <option value="">All sources</option>
          <option value="network">Network</option>
          <option value="logs">Logs</option>
          <option value="marker">Markers</option>
        </select>
        <select
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
        </select>
      </div>
      <div className="reading-controls">
        <label className="inline">
          <input
            type="checkbox"
            checked={reading.follow}
            onChange={(event) => {
              if (event.target.checked) reset({ through: null });
              setReading({
                ...reading,
                follow: event.target.checked,
                order: event.target.checked ? "newest_first" : reading.order,
              });
            }}
          />
          Follow latest
        </label>
        <label className="inline">
          <input
            type="checkbox"
            checked={reading.wrap}
            onChange={(event) =>
              setReading({ ...reading, wrap: event.target.checked })
            }
          />
          Wrap lines
        </label>
        <button
          className="text"
          disabled={!inspected}
          onClick={() => fence("clear")}
        >
          Clear view
        </button>
        <button
          className="text"
          disabled={!inspected}
          onClick={() => fence("full")}
        >
          Full session
        </button>
        <span className="muted">
          {paused
            ? `Paused through event ${query.through}. Capture continues.`
            : query.after !== null
              ? `Showing events after ${query.after}. Evidence remains saved.`
              : "Display controls do not change evidence."}
        </span>
      </div>
      {invalidation && (
        <div className="notice" role="status">
          {invalidation}{" "}
          <button className="text" onClick={() => setInvalidation("")}>
            Dismiss
          </button>
        </div>
      )}
      {error && (
        <div className="notice" role="alert">
          {error}
          {query.cursor === null && (
            <span>
              {" "}
              The first page will reload. Your filters are preserved.
            </span>
          )}
        </div>
      )}
      {page && (
        <div className="index-progress" role="status">
          <span>
            {page.progress.complete
              ? "Index up to date"
              : `Indexing · ${formatBytes(page.progress.indexed_bytes)} of ${formatBytes(page.progress.raw_bytes)} · ${formatBytes(page.progress.backlog_bytes)} pending`}
          </span>
          {!page.progress.complete && (
            <progress
              max={Math.max(page.progress.raw_bytes, 1)}
              value={page.progress.indexed_bytes}
              aria-label="Indexing evidence"
            />
          )}
        </div>
      )}
      <div
        className={`timeline-list ${reading.wrap ? "wrap" : "nowrap"}`}
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
          <div className="empty">
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
                <button
                  aria-pressed={visibleInspector?.eventId === event.id}
                  aria-haspopup="dialog"
                  className={`event ${visibleInspector?.eventId === event.id ? "selected" : ""}`}
                  onClick={(click) => void select(event.id, click.currentTarget)}
                >
                  <span className="event-time" title={event.timestamp}>
                    {new Date(event.timestamp).toLocaleTimeString()}
                    <small>#{sequence}</small>
                  </span>
                  <span className={`source ${event.source}`}>
                    {event.source === "network"
                      ? "NET"
                      : event.source === "marker"
                        ? "MARK"
                        : "LOG"}
                  </span>
                  <span className="event-text">
                    <strong>{event.kind.replaceAll("_", " ")}</strong>
                    <span>{event.summary}</span>
                  </span>
                </button>
              </div>
            ))}
          </div>
        )}
      </div>
      <div className="pagination">
        <button
          disabled={!query.previous.length || loading}
          onClick={() => {
            const history = query.previous.slice(0, -1);
            const cursor = query.previous.at(-1) ?? null;
            setQuery({ ...query, cursor, previous: history });
            setReading({ ...reading, follow: false });
          }}
        >
          {reading.order === "newest_first" ? "Newer page" : "Earlier page"}
        </button>
        <span>
          {page?.rows.length ?? 0} events on this page · page{" "}
          {query.previous.length + 1}
        </span>
        <button
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
        </button>
      </div>
      {visibleInspector && (
        <aside
          className="event-inspector"
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
          <div className="inspector-heading">
            <h2 id="event-inspector-title">Event details</h2>
            <button ref={closeButton} onClick={closeInspector}>
              Close event details
            </button>
          </div>
          <div className="inspector-content">
            {visibleInspector.kind === "loading" && (
              <p role="status">Reading event…</p>
            )}
            {visibleInspector.kind === "error" && (
              <p className="notice" role="alert">{visibleInspector.message}</p>
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
