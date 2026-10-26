import { Progress } from "./components/ui/progress";
import { Badge } from "./components/ui/badge";
import { Checkbox } from "./components/ui/checkbox";
import { Button } from "./components/ui/button";
import { Input } from "./components/ui/input";
import { Label } from "./components/ui/label";
import { NativeSelect } from "./components/ui/native-select";
import { formatBytes, logLevelSchema, type SessionDescriptor } from "./product";
import { Details } from "./Details";
import { EventSummary } from "./EventSummary";
import { useTimelineReader } from "./useTimelineReader";
import { X } from "lucide-react";

export function Timeline({
  inspected,
  visible = true,
}: {
  inspected: SessionDescriptor | undefined;
  visible?: boolean;
}) {
  const {
    reading,
    setReading,
    query,
    page,
    shownPageNumber,
    visibleInspector,
    error,
    invalidation,
    setInvalidation,
    loading,
    paused,
    canPrevious,
    canNext,
    firstPage,
    heading,
    list,
    closeButton,
    rows,
    select,
    rowKey,
    changePage,
    reset,
    fence,
    latest,
    closeInspector,
    selectionOffPage,
    inspectorControls,
  } = useTimelineReader({ inspected, visible });
  return (
    <section
      className="relative flex min-h-[16rem] min-w-0 flex-1 overflow-hidden"
      aria-label="Captured evidence"
    >
      <div className="flex min-h-0 min-w-0 flex-1 flex-col">
        <div className="flex shrink-0 flex-wrap items-center justify-between gap-2 px-4 pt-2 [&>div:first-child]:flex [&>div:first-child]:flex-wrap [&>div:first-child]:items-baseline [&>div:first-child]:gap-3 [&_h1]:text-base [&_h1]:font-medium">
          <div>
            <h1 ref={heading} tabIndex={-1}>
              Timeline
            </h1>
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
            aria-label="Log level filter"
            value={query.logLevel ?? ""}
            disabled={!inspected}
            onChange={(event) => {
              const value = event.target.value;
              const level = logLevelSchema.safeParse(value);
              if (value === "" || level.success)
                reset({ logLevel: level.success ? level.data : null });
            }}
          >
            <option value="">All log levels</option>
            <option value="V">Verbose</option>
            <option value="D">Debug</option>
            <option value="I">Info</option>
            <option value="W">Warning</option>
            <option value="E">Error</option>
            <option value="F">Fatal</option>
            <option value="A">Assert</option>
            <option value="S">Silent</option>
            <option value="unknown">Unknown</option>
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
            {firstPage && (
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
                    aria-keyshortcuts="ArrowUp ArrowDown Enter"
                    title="↑/↓ browse · Enter details"
                    aria-pressed={visibleInspector?.eventId === event.id}
                    aria-haspopup="dialog"
                    variant="ghost"
                    className={`grid h-auto min-h-16 w-full grid-cols-[70px_48px_minmax(0,1fr)] items-start justify-start gap-3 rounded-none border-0 border-b border-border px-4 py-3 text-left font-normal whitespace-normal select-text ${visibleInspector?.eventId === event.id ? "bg-accent hover:bg-accent" : "hover:bg-muted/50"}`}
                    ref={(element) => {
                      if (element) rows.current.set(event.id, element);
                      else rows.current.delete(event.id);
                    }}
                    onKeyDown={(key) => rowKey(key, event.id)}
                    onClick={() =>
                      void select(event.id, "timeline", event.source)
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
                    <EventSummary event={event} />
                  </Button>
                </div>
              ))}
            </div>
          )}
        </div>
        <div className="flex shrink-0 flex-wrap items-center justify-between gap-2 border-t border-border px-4 py-2 text-xs tabular-nums [&_button]:h-7 [&_button]:text-xs">
          <Button disabled={!canPrevious} onClick={() => changePage(-1)}>
            {reading.order === "newest_first" ? "Newer page" : "Earlier page"}
          </Button>
          <span>
            {page?.rows.length ?? 0} events on this page · page{" "}
            {shownPageNumber}
          </span>
          <Button disabled={!canNext} onClick={() => changePage(1)}>
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
            if (visible && event.key === "Escape") {
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
          <div className="flex min-h-0 flex-1 flex-col">
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
                visible={visible}
                inspector={inspectorControls}
              />
            )}
          </div>
        </aside>
      )}
    </section>
  );
}
