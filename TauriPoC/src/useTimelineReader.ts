import { useEffect, useRef, useState, type KeyboardEvent } from "react";
import { errorMessage } from "./api";
import {
  product,
  type LogLevel,
  type EventDetail,
  type SessionDescriptor,
  type TimelinePage,
  type TimelineQuery,
} from "./product";
import { timelineFence, useReading } from "./state";
import {
  initialInspectorPreferences,
  type FindFocusIntent,
} from "./inspector-state";

type Position =
  | { kind: "live"; cursor: string | null; previous: (string | null)[] }
  | {
      kind: "frozen";
      sessionKey: string;
      generation: string;
      eventId: string;
      relation: "at" | "before" | "after";
      pageNumber: number;
    };
type QueryState = {
  source: string;
  logLevel: LogLevel | null;
  search: string;
  position: Position;
  after: number | null;
  through: number | null;
  revision: number;
};
type InspectorEvent = {
  directory: SessionDescriptor["directory"];
  eventId: EventDetail["event"]["id"];
  source: string;
};
type Inspector =
  | { kind: "closed" }
  | (InspectorEvent &
      (
        | { kind: "loading" }
        | { kind: "ready"; detail: EventDetail }
        | { kind: "error"; message: string }
      ));
type FocusIntent =
  | { kind: "timeline" }
  | { kind: "details"; eventId: string }
  | ({ kind: "find" } & FindFocusIntent);
type PendingMove = {
  revision: number;
  anchor: string;
  direction: -1 | 1;
  kind: "freeze" | "edge";
};
const initialQuery: QueryState = {
  source: "",
  logLevel: null,
  search: "",
  position: { kind: "live", cursor: null, previous: [] },
  after: null,
  through: null,
  revision: 0,
};
function pageNumber(position: Position) {
  return position.kind === "live"
    ? position.previous.length + 1
    : position.pageNumber;
}
export function useTimelineReader({
  inspected,
  visible = true,
}: {
  inspected: SessionDescriptor | undefined;
  visible?: boolean;
}) {
  const [reading, setReading] = useReading();
  const [query, setQuery] = useState<QueryState>(initialQuery);
  const [page, setPage] = useState<TimelinePage>();
  const [shownPageNumber, setShownPageNumber] = useState(1);
  const [inspector, setInspector] = useState<Inspector>({ kind: "closed" });
  const [preferences, setPreferences] = useState(initialInspectorPreferences);
  const [focusIntent, setFocusIntent] = useState<FocusIntent>({
    kind: "timeline",
  });
  const focusOwner = useRef(focusIntent);
  const [error, setError] = useState("");
  const [invalidation, setInvalidation] = useState("");
  const [loading, setLoading] = useState(false);
  const generation = useRef(0);
  const detailGeneration = useRef(0);
  const context = useRef(0);
  const busy = useRef(false);
  const pending = useRef<PendingMove | null>(null);
  const acceptedPosition = useRef<{
    position: Position;
    through: number | null;
    context: number;
    directory: string;
  } | null>(null);
  const findToken = useRef(0);
  const rows = useRef(new Map<string, HTMLButtonElement>());
  const selectedId = useRef<string | null>(null);
  const closeButton = useRef<HTMLButtonElement>(null);
  const heading = useRef<HTMLHeadingElement>(null);
  const directory = useRef(inspected?.directory);
  const isVisible = useRef(visible);
  const list = useRef<HTMLDivElement>(null);
  const follow = useRef(reading.follow);
  const paused = query.through !== null;
  follow.current = reading.follow;
  directory.current = inspected?.directory;
  isVisible.current = visible;
  const visibleInspector =
    inspector.kind !== "closed" && inspector.directory === inspected?.directory
      ? inspector
      : undefined;
  function claimFocus(intent: FocusIntent) {
    focusOwner.current = intent;
    setFocusIntent(intent);
  }
  function cancelPending({ details = true } = {}) {
    context.current += 1;
    generation.current += 1;
    if (details) detailGeneration.current += 1;
    pending.current = null;
    busy.current = false;
    claimFocus({ kind: "timeline" });
  }
  function supersedeMovement() {
    if (!pending.current) return;
    const accepted = acceptedPosition.current;
    const position =
      pending.current.kind === "freeze" ? query.position : accepted?.position;
    cancelPending({ details: false });
    if (!accepted || !position) return;
    acceptedPosition.current = {
      ...accepted,
      position,
      through: query.through,
      context: context.current,
    };
    setQuery((previous) => ({
      ...previous,
      position,
      revision: previous.revision + 1,
    }));
  }
  useEffect(() => {
    cancelPending();
    setQuery(initialQuery);
    setPage(undefined);
    acceptedPosition.current = null;
    setShownPageNumber(1);
    setInspector({ kind: "closed" });
    setPreferences(initialInspectorPreferences);
    selectedId.current = null;
    setInvalidation("");
    return () => {
      generation.current += 1;
      detailGeneration.current += 1;
    };
  }, [inspected?.directory]);
  useEffect(() => {
    if (!visible) {
      const accepted = acceptedPosition.current;
      if (busy.current && accepted)
        setQuery((previous) => ({
          ...previous,
          position: accepted.position,
          through: accepted.through,
          revision: previous.revision + 1,
        }));
      cancelPending({ details: false });
    }
  }, [visible]);
  useEffect(() => {
    if (
      visible &&
      visibleInspector &&
      focusIntent.kind === "details" &&
      focusIntent.eventId === visibleInspector.eventId
    )
      closeButton.current?.focus();
  }, [visible, visibleInspector?.eventId, focusIntent]);
  useEffect(() => {
    if (!visible || !visibleInspector || visibleInspector.source !== "network")
      return;
    function key(event: globalThis.KeyboardEvent) {
      if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === "f") {
        event.preventDefault();
        event.stopPropagation();
        requestFind();
      }
    }
    document.addEventListener("keydown", key, true);
    return () => document.removeEventListener("keydown", key, true);
  }, [visible, visibleInspector, page?.session_key, query]);
  function requestFind() {
    if (
      !visibleInspector ||
      !visible ||
      visibleInspector.source !== "network" ||
      !page
    )
      return;
    supersedeMovement();
    setPreferences((previous) => ({
      ...previous,
      find: { ...previous.find, open: true },
    }));
    claimFocus({
      kind: "find",
      event: {
        session_key:
          visibleInspector.kind === "ready"
            ? visibleInspector.detail.session_key
            : page.session_key,
        event_id: visibleInspector.eventId,
      },
      token: ++findToken.current,
    });
  }
  function focusRow(id: string) {
    const row = rows.current.get(id);
    if (!isVisible.current || !row) return;
    row.focus({ preventScroll: true });
    row.scrollIntoView({ block: "nearest" });
  }
  async function select(
    eventId: string,
    intent: "timeline" | "details",
    source?: string,
  ) {
    if (!inspected) return;
    supersedeMovement();
    const selected = {
      directory: inspected.directory,
      eventId,
      source:
        source ??
        page?.rows.find((row) => row.event.id === eventId)?.event.source ??
        "",
    };
    const request = ++detailGeneration.current;
    selectedId.current = eventId;
    if (intent === "timeline") focusRow(eventId);
    claimFocus(
      intent === "timeline"
        ? { kind: "timeline" }
        : { kind: "details", eventId },
    );
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
  function installMove(result: TimelinePage, movement: PendingMove) {
    const anchorIndex = result.rows.findIndex(
      (row) => row.event.id === movement.anchor,
    );
    const index =
      movement.kind === "freeze"
        ? anchorIndex + movement.direction
        : movement.direction === 1
          ? 0
          : result.rows.length - 1;
    const target =
      movement.kind === "freeze" && anchorIndex === -1
        ? undefined
        : result.rows[index];
    pending.current = null;
    busy.current = false;
    if (target) {
      void select(target.event.id, "timeline", target.event.source);
      scheduleFocus(target.event.id);
    } else if (movement.kind === "freeze" && anchorIndex !== -1) {
      const anchor = result.rows[anchorIndex];
      void select(anchor.event.id, "timeline", anchor.event.source);
      scheduleFocus(anchor.event.id);
      if (movement.direction === 1 ? result.has_next : result.has_previous)
        changePage(movement.direction, result, movement);
    }
  }
  function scheduleFocus(id: string) {
    const requestContext = context.current;
    const owner = focusOwner.current;
    requestAnimationFrame(() => {
      if (
        requestContext === context.current &&
        owner === focusOwner.current &&
        selectedId.current === id
      )
        focusRow(id);
    });
  }
  useEffect(() => {
    const request = ++generation.current;
    const requestContext = context.current;
    let disposed = false;
    let timer: ReturnType<typeof setTimeout>;
    if (!inspected || !visible) return;
    const requestDirectory = inspected.directory;
    const position = query.position;
    const input: TimelineQuery = {
      source: query.source || null,
      log_level: query.logLevel,
      search: query.search,
      order: reading.order,
      limit: 200,
      cursor: position.kind === "live" ? position.cursor : null,
      seek:
        position.kind === "frozen"
          ? {
              session_key: position.sessionKey,
              generation: position.generation,
              event_id: position.eventId,
              relation: position.relation,
            }
          : null,
      after_sequence: query.after,
      through_sequence: query.through,
    };
    setLoading(true);
    setError("");
    async function poll() {
      try {
        const result = await product.query(input);
        if (
          disposed ||
          request !== generation.current ||
          requestContext !== context.current ||
          requestDirectory !== directory.current ||
          !isVisible.current
        )
          return;
        if (
          position.kind === "frozen" &&
          (result.session_key !== position.sessionKey ||
            result.generation !== position.generation)
        )
          throw new Error(
            "The frozen timeline belongs to a different session or generation.",
          );
        setPage(result);
        acceptedPosition.current = {
          position,
          through: query.through,
          context: requestContext,
          directory: requestDirectory,
        };
        setShownPageNumber(pageNumber(position));
        setError("");
        setLoading(false);
        busy.current = false;
        const movement = pending.current;
        if (movement?.revision === query.revision)
          installMove(result, movement);
        if (
          follow.current &&
          position.kind === "live" &&
          position.cursor === null &&
          query.through === null &&
          list.current
        )
          list.current.scrollTop =
            reading.order === "newest_first" ? 0 : list.current.scrollHeight;
      } catch (cause) {
        if (
          disposed ||
          request !== generation.current ||
          requestContext !== context.current
        )
          return;
        setError(errorMessage(cause));
        setLoading(false);
        pending.current = null;
        busy.current = false;
        if (position.kind === "frozen") {
          setInvalidation(
            "The frozen view could not be read. Your displayed page and filters are preserved. Resume or jump to latest to reload.",
          );
          return;
        }
        if (input.cursor) {
          setInvalidation(
            `The previous page could not be read. ${errorMessage(cause)} The first page has reloaded with your filters preserved.`,
          );
          setQuery((previous) => ({
            ...previous,
            position: { kind: "live", cursor: null, previous: [] },
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
  }, [inspected?.directory, query, reading.order, visible]);
  function closeInspector() {
    supersedeMovement();
    detailGeneration.current += 1;
    setInspector({ kind: "closed" });
    claimFocus({ kind: "timeline" });
    const target = selectedId.current && rows.current.get(selectedId.current);
    if (target) target.focus();
    else heading.current?.focus();
  }
  function reset(changes: Partial<QueryState>) {
    cancelPending();
    acceptedPosition.current = null;
    setInspector((previous) =>
      previous.kind === "loading" ? { kind: "closed" } : previous,
    );
    setInvalidation("");
    setQuery((previous) => ({
      ...previous,
      ...changes,
      position: { kind: "live", cursor: null, previous: [] },
      revision: previous.revision + 1,
    }));
  }
  function changePage(
    direction: -1 | 1,
    current = page,
    movement?: PendingMove,
  ) {
    const accepted = acceptedPosition.current;
    if (
      !current ||
      busy.current ||
      !accepted ||
      accepted.context !== context.current ||
      accepted.directory !== inspected?.directory
    )
      return;
    const position = query.position;
    if (position.kind === "frozen") {
      if (direction === 1 ? !current.has_next : !current.has_previous) return;
      const anchor = direction === 1 ? current.rows.at(-1) : current.rows[0];
      if (!anchor) return;
      const revision = query.revision + 1;
      busy.current = true;
      generation.current += 1;
      pending.current = movement
        ? { ...movement, kind: "edge", revision }
        : null;
      const number = Math.max(1, position.pageNumber + direction);
      if (direction === -1 && position.pageNumber === 1)
        setInvalidation(
          "Earlier events moved the frozen position. Page numbering continues from 1.",
        );
      setQuery({
        ...query,
        revision,
        position: {
          ...position,
          eventId: anchor.event.id,
          relation: direction === 1 ? "after" : "before",
          pageNumber: number,
        },
      });
    } else if (direction === 1 && current.next_cursor) {
      busy.current = true;
      generation.current += 1;
      setQuery({
        ...query,
        revision: query.revision + 1,
        position: {
          kind: "live",
          cursor: current.next_cursor,
          previous: [...position.previous, position.cursor],
        },
      });
    } else if (direction === -1 && position.previous.length) {
      busy.current = true;
      generation.current += 1;
      setQuery({
        ...query,
        revision: query.revision + 1,
        position: {
          kind: "live",
          cursor: position.previous.at(-1) ?? null,
          previous: position.previous.slice(0, -1),
        },
      });
    }
    setReading({ ...reading, follow: false });
  }
  function rowKey(event: KeyboardEvent<HTMLButtonElement>, id: string) {
    if (
      !visible ||
      event.target !== event.currentTarget ||
      document.activeElement !== event.currentTarget ||
      event.altKey ||
      event.ctrlKey ||
      event.metaKey ||
      event.shiftKey
    )
      return;
    if (event.key === "Enter") {
      event.preventDefault();
      void select(id, "details");
      return;
    }
    if (event.key === "Escape" && visibleInspector) {
      event.preventDefault();
      closeInspector();
      return;
    }
    if (event.key !== "ArrowUp" && event.key !== "ArrowDown") return;
    event.preventDefault();
    claimFocus({ kind: "timeline" });
    const accepted = acceptedPosition.current;
    if (
      loading ||
      busy.current ||
      !page?.rows.length ||
      !accepted ||
      accepted.context !== context.current ||
      accepted.directory !== inspected?.directory ||
      (query.position.kind === "frozen" && error)
    )
      return;
    const direction = event.key === "ArrowDown" ? 1 : -1;
    const index = page.rows.findIndex((row) => row.event.id === id);
    if (index === -1) return;
    if (query.position.kind === "live") {
      busy.current = true;
      generation.current += 1;
      const revision = query.revision + 1;
      pending.current = { kind: "freeze", revision, anchor: id, direction };
      setReading({ ...reading, follow: false });
      setQuery({
        ...query,
        revision,
        through: query.through ?? page.progress.latest_sequence,
        position: {
          kind: "frozen",
          sessionKey: page.session_key,
          generation: page.generation,
          eventId: page.rows[0].event.id,
          relation: "at",
          pageNumber: pageNumber(query.position),
        },
      });
      return;
    }
    const target = page.rows[index + direction];
    if (target) {
      focusRow(target.event.id);
      void select(target.event.id, "timeline", target.event.source);
    } else
      changePage(direction, page, {
        kind: "edge",
        revision: query.revision,
        anchor: id,
        direction,
      });
  }
  function fence(action: "pause" | "clear" | "full") {
    reset(timelineFence(action, page?.progress.latest_sequence ?? 0, query));
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
  return {
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
    canPrevious:
      !loading &&
      (query.position.kind === "frozen"
        ? !error && !!page?.has_previous
        : query.position.previous.length > 0),
    canNext:
      !loading &&
      (query.position.kind === "frozen"
        ? !error && !!page?.has_next
        : !!page?.next_cursor),
    firstPage: query.position.kind === "live" && query.position.cursor === null,
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
    inspectorControls: {
      preferences,
      onPreferences: setPreferences,
      findFocus: focusIntent.kind === "find" ? focusIntent : null,
      onFindFocusConsumed: (token: number) =>
        setFocusIntent((previous) =>
          previous.kind === "find" && previous.token === token
            ? { kind: "timeline" }
            : previous,
        ),
      onRequestFind: requestFind,
    },
  };
}
