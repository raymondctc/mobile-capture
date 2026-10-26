import { useEffect, useRef, useState } from "react";
import { Button } from "./components/ui/button";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "./components/ui/dropdown-menu";
import { ToggleGroup, ToggleGroupItem } from "./components/ui/toggle-group";
import { ChevronDown } from "lucide-react";
import { Input } from "./components/ui/input";
import { DetailChunks } from "./DetailChunks";
import { QueryParameters } from "./QueryParameters";
import { Headers } from "./Headers";
import type { InspectorPreferenceProps } from "./inspector-state";
import { BodyWindow } from "./BodyViewport";
import {
  type Representation,
  type Search,
  type StableMatch,
} from "./body-document";
import { bodyDocuments, type DocumentOwner } from "./body-owner";
import { type Reply, type DocumentInfo, type Window } from "./body-session";
import { errorMessage } from "./api";
import { product, type BodySide, type EventRef } from "./product";

type State =
  | { kind: "loading" }
  | {
      kind: "ready";
      documents: DocumentInfo;
      query: "present" | "absent" | "unavailable";
      headers: Extract<Reply, { kind: "loaded" }>["headers"];
    }
  | { kind: "error"; message: string };
type Find =
  | { kind: "closed" }
  | { kind: "searching" }
  | { kind: "complete"; search: Search }
  | { kind: "error"; message: string };
type Copy =
  | { kind: "idle" }
  | { kind: "pending" }
  | { kind: "done"; mode: Representation }
  | { kind: "error"; message: string };
const sides: BodySide[] = ["request", "response"];
export function BodyInspector({
  reference,
  wrap,
  visible,
  paneVisible,
  onRaw,
  preferences,
  onPreferences,
  findFocus,
  onFindFocusConsumed,
  onRequestFind,
}: {
  reference: EventRef;
  wrap: boolean;
  visible: boolean;
  paneVisible: boolean;
  onRaw: (side: BodySide) => void;
} & InspectorPreferenceProps) {
  const [state, setState] = useState<State>({ kind: "loading" });
  const side = preferences.side ?? "response";
  const mode = preferences.representation;
  const [offset, setOffset] = useState(0);
  const [find, setFind] = useState<Find>({ kind: "closed" });
  const [window, setWindow] = useState<
    | { kind: "loading" }
    | { kind: "ready"; value: Window | null }
    | { kind: "error"; message: string }
  >({ kind: "loading" });
  const owner = useRef<DocumentOwner | null>(null);
  const findRevision = useRef(0);
  const [copy, setCopy] = useState<Copy>({ kind: "idle" });
  const input = useRef<HTMLInputElement>(null);
  const findButton = useRef<HTMLButtonElement>(null);
  const revision = useRef(0);
  useEffect(() => {
    const request = ++revision.current;
    setState({ kind: "loading" });
    setFind({ kind: "closed" });
    setCopy({ kind: "idle" });
    setOffset(0);
    let document: DocumentOwner;
    try {
      document = bodyDocuments.create();
    } catch (cause) {
      setState({ kind: "error", message: errorMessage(cause) });
      return;
    }
    owner.current = document;
    void product
      .bodyDocuments(reference)
      .then((pair) => document.read({ kind: "load", pair }))
      .then((reply) => {
        if (request !== revision.current) return;
        if (reply.kind !== "loaded")
          throw new Error("Unexpected body document reply");
        onPreferences((previous) =>
          previous.side === null ? { ...previous, side: reply.side } : previous,
        );
        setState({
          kind: "ready",
          documents: reply.documents,
          query: reply.query,
          headers: reply.headers,
        });
      })
      .catch((cause: unknown) => {
        if (request === revision.current)
          setState({ kind: "error", message: errorMessage(cause) });
      });
    return () => {
      revision.current += 1;
      findRevision.current += 1;
      document.dispose();
      owner.current = null;
    };
  }, [reference.session_key, reference.event_id, onPreferences]);
  const query = preferences.find.open ? preferences.find.query : "";
  const searching = find.kind === "searching";
  const findOpen = preferences.find.open;
  useEffect(() => {
    if (state.kind === "ready" && findOpen)
      void search(query, { reveal: false, ordinal: -1 });
  }, [state.kind, findOpen]);
  useEffect(() => {
    if (state.kind !== "ready" || !owner.current) return;
    let disposed = false;
    setWindow({ kind: "loading" });
    if (searching) return;
    void owner.current
      .read({ kind: "window", side, mode, query, offset })
      .then((reply) => {
        if (disposed) return;
        if (reply.kind !== "window")
          throw new Error("Unexpected body window reply");
        setWindow({ kind: "ready", value: reply.window });
      })
      .catch((cause: unknown) => {
        if (!disposed)
          setWindow({ kind: "error", message: errorMessage(cause) });
      });
    return () => {
      disposed = true;
    };
  }, [state.kind, side, mode, query, offset, searching]);
  useEffect(() => {
    if (
      findFocus &&
      findFocus.event.session_key === reference.session_key &&
      findFocus.event.event_id === reference.event_id &&
      findOpen &&
      visible &&
      paneVisible &&
      state.kind === "ready" &&
      input.current
    ) {
      input.current.focus();
      onFindFocusConsumed(findFocus.token);
    }
  }, [
    findFocus,
    findOpen,
    visible,
    paneVisible,
    state.kind,
    reference.session_key,
    reference.event_id,
    onFindFocusConsumed,
  ]);
  function closeFind() {
    findRevision.current += 1;
    onPreferences((previous) => ({
      ...previous,
      find: { ...previous.find, open: false },
    }));
    setFind({ kind: "closed" });
    findButton.current?.focus();
  }
  useEffect(() => {
    if (!visible || !paneVisible || !findOpen) return;
    function key(event: KeyboardEvent) {
      if (
        event.key === "Escape" &&
        !(
          event.target instanceof Element &&
          event.target.closest('[role="menu"]')
        )
      ) {
        event.preventDefault();
        event.stopPropagation();
        closeFind();
      }
    }
    document.addEventListener("keydown", key, true);
    return () => document.removeEventListener("keydown", key, true);
  }, [visible, paneVisible, findOpen]);
  function showMatch(search: Search) {
    const active = search.active;
    if (!active) return;
    const side =
      active.region.kind === "query" ? "request" : active.region.side;
    onPreferences((previous) => ({
      ...previous,
      side,
      headersOpen:
        active.region.kind === "header"
          ? { ...previous.headersOpen, [side]: true }
          : previous.headersOpen,
    }));
    if (active.region.kind === "body")
      setOffset(Math.max(0, active.start - 200));
  }
  async function search(
    query: string,
    {
      representation = mode,
      ordinal = 0,
      debounce = false,
      preserveSide,
      preferred,
      reveal = true,
    }: {
      representation?: Representation;
      ordinal?: number;
      debounce?: boolean;
      preserveSide?: BodySide;
      preferred?: StableMatch;
      reveal?: boolean;
    } = {},
  ) {
    onPreferences((previous) => ({ ...previous, find: { open: true, query } }));
    if (state.kind !== "ready" || !owner.current) return;
    const request = ++findRevision.current;
    setFind({ kind: "searching" });
    try {
      if (debounce) await new Promise((resolve) => setTimeout(resolve, 150));
      if (request !== findRevision.current || !owner.current) return;
      let reply = await owner.current.read({
        kind: "find",
        mode: representation,
        query,
        ordinal,
        preferred,
      });
      if (request !== findRevision.current || !owner.current) return;
      if (reply.kind !== "found") throw new Error("Unexpected body Find reply");
      if (
        preserveSide &&
        (reply.search.active?.region.kind === "query"
          ? "request"
          : reply.search.active?.region.side) !== preserveSide
      ) {
        const first =
          preserveSide === "request"
            ? 0
            : reply.search.requestHeaders +
              reply.search.queryParameters +
              reply.search.request;
        const count =
          reply.search[preserveSide] +
          (preserveSide === "request"
            ? reply.search.requestHeaders + reply.search.queryParameters
            : reply.search.responseHeaders);
        if (count) {
          reply = await owner.current.read({
            kind: "find",
            mode: representation,
            query,
            ordinal: first,
          });
          if (request !== findRevision.current) return;
          if (reply.kind !== "found")
            throw new Error("Unexpected body Find reply");
        } else reply = { ...reply, search: { ...reply.search, active: null } };
      }
      setFind({ kind: "complete", search: reply.search });
      if (reveal) showMatch(reply.search);
    } catch (cause) {
      if (request === findRevision.current)
        setFind({ kind: "error", message: errorMessage(cause) });
    }
  }
  function navigate(direction: 1 | -1) {
    if (find.kind !== "complete" || !find.search.total) return;
    const current = find.search.active?.ordinal ?? (direction === 1 ? -1 : 0);
    void search(query, {
      ordinal: (current + direction + find.search.total) % find.search.total,
    });
  }
  async function copyBody(representation: Representation) {
    if (state.kind !== "ready" || !owner.current) return;
    const request = revision.current;
    setCopy({ kind: "pending" });
    try {
      const reply = await owner.current.read({
        kind: "copy",
        side,
        mode: representation,
      });
      if (reply.kind !== "copy") throw new Error("Unexpected body copy reply");
      if (request !== revision.current) return;
      await product.copyText(reply.text);
      if (request === revision.current)
        setCopy({ kind: "done", mode: representation });
    } catch (cause) {
      if (request === revision.current)
        setCopy({ kind: "error", message: errorMessage(cause) });
    }
  }
  if (state.kind === "loading")
    return <p role="status">Reading complete request and response bodies…</p>;
  if (state.kind === "error") return <p role="alert">{state.message}</p>;
  const body = state.documents[side];
  const formattedAvailable =
    body.kind === "text" && body.formatting.kind === "ready";
  const displayMode = formattedAvailable ? mode : "original";
  const defaultCopyMode = formattedAvailable ? "formatted" : "original";
  const queryOnly =
    side === "request" &&
    state.query === "present" &&
    body.kind === "text" &&
    body.bytes === 0;
  return (
    <div className="space-y-3">
      <div className="flex flex-wrap items-center gap-2">
        <ToggleGroup
          type="single"
          aria-label="Network bodies"
          value={side}
          onValueChange={(value) => {
            if (value !== "request" && value !== "response") return;
            onPreferences((previous) => ({ ...previous, side: value }));
            setOffset(0);
            setCopy({ kind: "idle" });
          }}
        >
          {sides.map((value) => {
            const searchable =
              state.documents[value].kind === "text" ||
              (value === "request" && state.query === "present") ||
              state.headers[value].kind === "present";
            const count =
              find.kind === "complete"
                ? find.search[value] +
                  (value === "request"
                    ? find.search.requestHeaders + find.search.queryParameters
                    : find.search.responseHeaders)
                : "…";
            return (
              <ToggleGroupItem key={value} value={value}>
                {value === "request" ? "Request" : "Response"}
                {findOpen && query
                  ? ` (${searchable ? count : "not searchable"})`
                  : ""}
              </ToggleGroupItem>
            );
          })}
        </ToggleGroup>
        <Button ref={findButton} onClick={onRequestFind}>
          Find
        </Button>
      </div>
      {sides.map((value) => {
        const document = state.documents[value];
        return document.kind === "unavailable" ? (
          <p key={value} className="text-xs text-muted-foreground">
            {value === "request" ? "Request" : "Response"} unavailable ·{" "}
            {document.reason}
          </p>
        ) : null;
      })}
      {findOpen && (
        <div
          className="space-y-2 rounded-md border border-border p-2"
          role="search"
          aria-label="Find in headers, query and both bodies"
        >
          <Input
            ref={input}
            aria-label="Find in headers, query, request and response"
            value={query}
            maxLength={256}
            placeholder="Literal text, case insensitive"
            onChange={(event) =>
              void search(event.target.value, { debounce: true })
            }
            onKeyDown={(event) => {
              if (event.key === "Enter") {
                event.preventDefault();
                navigate(event.shiftKey ? -1 : 1);
              }
            }}
          />
          <div className="flex flex-wrap items-center gap-2 text-xs">
            <span role="status">
              {find.kind === "complete"
                ? `${find.search.active ? find.search.active.ordinal + 1 : 0} of ${find.search.total} matches · Request headers ${find.search.requestHeaders} · Query parameters ${find.search.queryParameters} · Request body ${state.documents.request.kind === "text" ? find.search.request : "not searchable"} · Response headers ${find.search.responseHeaders} · Response body ${state.documents.response.kind === "text" ? find.search.response : "not searchable"}`
                : find.kind === "searching"
                  ? "Searching complete headers, query and bodies…"
                  : "Find failed"}
            </span>
            {find.kind === "error" && <p role="alert">{find.message}</p>}

            <Button
              disabled={find.kind !== "complete" || !find.search.total}
              onClick={() => navigate(-1)}
            >
              Previous match
            </Button>
            <Button
              disabled={find.kind !== "complete" || !find.search.total}
              onClick={() => navigate(1)}
            >
              Next match
            </Button>
            <Button onClick={closeFind}>Close Find</Button>
          </div>
        </div>
      )}
      <section
        aria-label={`${side === "request" ? "Request" : "Response"} headers`}
        className="space-y-2"
      >
        <Button
          aria-expanded={preferences.headersOpen[side]}
          onClick={() =>
            onPreferences((previous) => ({
              ...previous,
              headersOpen: {
                ...previous.headersOpen,
                [side]: !previous.headersOpen[side],
              },
            }))
          }
        >
          {side === "request" ? "Request" : "Response"} headers
          {state.headers[side].kind === "present"
            ? ` (${state.headers[side].total})`
            : ""}
        </Button>
        {preferences.headersOpen[side] &&
          (state.headers[side].kind === "present" && owner.current ? (
            <Headers
              key={side}
              owner={owner.current}
              side={side}
              query={query}
              active={find.kind === "complete" ? find.search.active : null}
              wrap={wrap}
            />
          ) : (
            <p>
              {state.headers[side].kind === "unavailable"
                ? `Headers unavailable. ${state.headers[side].reason}`
                : "Headers were not recorded."}
            </p>
          ))}
      </section>
      {side === "request" && state.query === "present" && owner.current && (
        <QueryParameters
          owner={owner.current}
          query={query}
          active={find.kind === "complete" ? find.search.active : null}
          wrap={wrap}
        />
      )}
      {side === "request" && state.query === "unavailable" && (
        <p>
          Query parameters unavailable. The complete request URL was not
          recorded.
        </p>
      )}
      <p className="text-xs text-muted-foreground">
        {body.media.label}
        {body.media.content_type ? ` · ${body.media.content_type}` : ""} ·{" "}
        {body.retained.kind === "available"
          ? `${body.retained.bytes.toLocaleString()} retained bytes`
          : "Retained bytes unavailable"}
        {body.kind !== "unavailable" && body.basis === "decoded"
          ? ` · ${body.bytes.toLocaleString()} decoded bytes`
          : ""}
        {body.media.content_encoding
          ? ` · Content encoding ${body.media.content_encoding}`
          : ""}
      </p>
      {body.kind === "text" && !queryOnly && (
        <>
          <div className="flex flex-wrap items-center gap-2">
            <ToggleGroup
              type="single"
              aria-label="Body representation"
              value={displayMode}
              onValueChange={(value) => {
                if (value !== "original" && value !== "formatted") return;
                onPreferences((previous) => ({
                  ...previous,
                  representation: value,
                }));
                setOffset(0);
                if (findOpen)
                  void search(query, {
                    representation: value,
                    preserveSide: side,
                    preferred:
                      find.kind === "complete" &&
                      find.search.active &&
                      find.search.active.region.kind !== "body"
                        ? {
                            region: find.search.active.region,
                            start: find.search.active.start,
                          }
                        : undefined,
                  });
              }}
            >
              <ToggleGroupItem value="original">Original</ToggleGroupItem>
              <ToggleGroupItem value="formatted" disabled={!formattedAvailable}>
                Formatted
              </ToggleGroupItem>
            </ToggleGroup>
            <div
              role="group"
              aria-label="Copy body"
              className="flex items-center"
            >
              <Button
                className="rounded-r-none focus-visible:z-10"
                disabled={copy.kind === "pending"}
                onClick={() => void copyBody(defaultCopyMode)}
              >
                Copy
              </Button>
              {visible && paneVisible && (
                <DropdownMenu modal={false}>
                  <DropdownMenuTrigger asChild>
                    <Button
                      className="-ml-px rounded-l-none focus-visible:z-10"
                      size="icon"
                      aria-label="Choose body copy format"
                      disabled={copy.kind === "pending"}
                    >
                      <ChevronDown aria-hidden="true" />
                    </Button>
                  </DropdownMenuTrigger>
                  <DropdownMenuContent
                    align="end"
                    className="w-40"
                    onEscapeKeyDown={(event) => event.stopPropagation()}
                    onCloseAutoFocus={(event) => {
                      if (
                        !findButton.current ||
                        findButton.current.closest("[hidden],[inert]")
                      )
                        event.preventDefault();
                    }}
                  >
                    <DropdownMenuItem
                      disabled={copy.kind === "pending" || !formattedAvailable}
                      onSelect={() => void copyBody("formatted")}
                    >
                      Copy formatted
                    </DropdownMenuItem>
                    <DropdownMenuItem
                      disabled={copy.kind === "pending"}
                      onSelect={() => void copyBody("original")}
                    >
                      Copy original
                    </DropdownMenuItem>
                  </DropdownMenuContent>
                </DropdownMenu>
              )}
            </div>
          </div>
          <p className="text-xs text-muted-foreground">
            {body.basis === "decoded"
              ? "Automatically decoded content"
              : "Retained UTF-8 content"}
            . Original preserves the complete unformatted text.
          </p>
          {body.formatting.kind === "unavailable" && (
            <p className="text-xs text-muted-foreground">
              {body.formatting.reason}
            </p>
          )}
        </>
      )}
      {body.kind !== "text" && <p role="status">{body.reason}</p>}
      {body.kind !== "text" && body.retained.kind === "available" && (
        <DetailChunks reference={reference} side={side} wrap={wrap} />
      )}
      {copy.kind === "error" && <p role="alert">Copy failed. {copy.message}</p>}
      {copy.kind === "done" && (
        <p role="status">Copied complete {copy.mode} body.</p>
      )}
      {window.kind === "loading" && body.kind === "text" && !queryOnly && (
        <p role="status">Reading body window…</p>
      )}
      {window.kind === "error" && <p role="alert">{window.message}</p>}
      {window.kind === "ready" && window.value && !queryOnly && (
        <BodyWindow
          window={window.value}
          onOffset={setOffset}
          wrap={wrap}
          active={
            find.kind === "complete" &&
            find.search.active?.region.kind === "body" &&
            find.search.active.region.side === side
              ? find.search.active
              : null
          }
        />
      )}

      {body.kind === "text" && (
        <Button
          variant="ghost"
          disabled={body.retained.kind !== "available"}
          onClick={() => onRaw(side)}
        >
          Retained bytes · hexadecimal
        </Button>
      )}
    </div>
  );
}
