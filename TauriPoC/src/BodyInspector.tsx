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
import { BodyWindow } from "./BodyViewport";
import { type Representation, type Search } from "./body-document";
import { bodyDocuments, type DocumentOwner } from "./body-owner";
import { type DocumentInfo, type Window } from "./body-session";
import { errorMessage } from "./api";
import { product, type BodySide, type EventRef } from "./product";

type State =
  | { kind: "loading" }
  | {
      kind: "ready";
      documents: DocumentInfo;
      query: "present" | "absent" | "unavailable";
    }
  | { kind: "error"; message: string };
type Find =
  | { kind: "closed" }
  | { kind: "searching"; query: string }
  | { kind: "complete"; query: string; search: Search }
  | { kind: "error"; query: string; message: string };
type Copy =
  | { kind: "idle" }
  | { kind: "pending" }
  | { kind: "done"; mode: Representation }
  | { kind: "error"; message: string };
const emptySearch: Search = {
  queryParameters: 0,
  request: 0,
  response: 0,
  total: 0,
  active: null,
};
const sides: BodySide[] = ["request", "response"];
export function BodyInspector({
  reference,
  wrap,
  visible,
  paneVisible,
  onFindOpen,
  onRaw,
}: {
  reference: EventRef;
  wrap: boolean;
  visible: boolean;
  paneVisible: boolean;
  onFindOpen: () => void;
  onRaw: (side: BodySide) => void;
}) {
  const [state, setState] = useState<State>({ kind: "loading" });
  const [side, setSide] = useState<BodySide>("response");
  const [mode, setMode] = useState<Representation>("formatted");
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
    setMode("formatted");
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
        setSide(reply.side);
        setState({
          kind: "ready",
          documents: reply.documents,
          query: reply.query,
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
  }, [reference.session_key, reference.event_id]);
  const query = find.kind !== "closed" ? find.query : "";
  const searching = find.kind === "searching";
  const findOpen = find.kind !== "closed";
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
    if (findOpen && visible && paneVisible && state.kind === "ready")
      input.current?.focus();
  }, [findOpen, visible, paneVisible, state.kind]);
  useEffect(() => {
    if (!visible) return;
    function key(event: KeyboardEvent) {
      if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === "f") {
        event.preventDefault();
        event.stopPropagation();
        onFindOpen();
        setFind((previous) =>
          previous.kind !== "closed"
            ? previous
            : { kind: "complete", query: "", search: emptySearch },
        );
        if (paneVisible) input.current?.focus();
      } else if (
        event.key === "Escape" &&
        !(
          event.target instanceof Element &&
          event.target.closest('[role="menu"]')
        ) &&
        find.kind !== "closed" &&
        paneVisible
      ) {
        event.preventDefault();
        event.stopPropagation();
        findRevision.current += 1;
        setFind({ kind: "closed" });
        findButton.current?.focus();
      }
    }
    document.addEventListener("keydown", key, true);
    return () => document.removeEventListener("keydown", key, true);
  }, [visible, paneVisible, find.kind, onFindOpen]);
  function showMatch(search: Search) {
    if (search.active) {
      setSide(
        search.active.region.kind === "query"
          ? "request"
          : search.active.region.side,
      );
      if (search.active.region.kind === "body")
        setOffset(Math.max(0, search.active.start - 200));
    }
  }
  async function search(
    query: string,
    {
      representation = mode,
      ordinal = 0,
      debounce = false,
      preserveSide,
    }: {
      representation?: Representation;
      ordinal?: number;
      debounce?: boolean;
      preserveSide?: BodySide;
    } = {},
  ) {
    if (state.kind !== "ready" || !owner.current) return;
    const preserveQuery =
      preserveSide === "request" &&
      find.kind === "complete" &&
      find.search.active?.region.kind === "query";
    const request = ++findRevision.current;
    setFind({ kind: "searching", query });
    try {
      if (debounce) await new Promise((resolve) => setTimeout(resolve, 150));
      if (request !== findRevision.current || !owner.current) return;
      let reply = await owner.current.read({
        kind: "find",
        mode: representation,
        query,
        ordinal:
          preserveQuery && find.kind === "complete" && find.search.active
            ? find.search.active.ordinal
            : ordinal,
      });
      if (request !== findRevision.current || !owner.current) return;
      if (reply.kind !== "found") throw new Error("Unexpected body Find reply");
      if (
        preserveSide &&
        !preserveQuery &&
        (reply.search.active?.region.kind !== "body" ||
          reply.search.active.region.side !== preserveSide)
      ) {
        if (reply.search[preserveSide]) {
          reply = await owner.current.read({
            kind: "find",
            mode: representation,
            query,
            ordinal:
              reply.search.queryParameters +
              (preserveSide === "response" ? reply.search.request : 0),
          });
          if (request !== findRevision.current) return;
          if (reply.kind !== "found")
            throw new Error("Unexpected body Find reply");
        } else {
          reply = { ...reply, search: { ...reply.search, active: null } };
        }
      }
      setFind({ kind: "complete", query, search: reply.search });
      showMatch(reply.search);
    } catch (cause) {
      if (request === findRevision.current) {
        setFind({ kind: "error", query, message: errorMessage(cause) });
      }
    }
  }
  function navigate(direction: 1 | -1) {
    if (find.kind !== "complete" || !find.search.total) return;
    const current = find.search.active?.ordinal ?? (direction === 1 ? -1 : 0);
    const ordinal =
      (current + direction + find.search.total) % find.search.total;
    void search(find.query, { ordinal });
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
            setSide(value);
            setOffset(0);
            setCopy({ kind: "idle" });
          }}
        >
          {sides.map((value) => {
            const searchable =
              state.documents[value].kind === "text" ||
              (value === "request" && state.query === "present");
            const count =
              find.kind === "complete"
                ? find.search[value] +
                  (value === "request" ? find.search.queryParameters : 0)
                : "…";
            return (
              <ToggleGroupItem key={value} value={value}>
                {value === "request" ? "Request" : "Response"}
                {find.kind !== "closed" && find.query
                  ? ` (${searchable ? count : "not searchable"})`
                  : ""}
              </ToggleGroupItem>
            );
          })}
        </ToggleGroup>
        <Button
          ref={findButton}
          onClick={() => {
            onFindOpen();
            setFind((previous) =>
              previous.kind !== "closed"
                ? previous
                : {
                    kind: "complete",
                    query: "",
                    search: emptySearch,
                  },
            );
            input.current?.focus();
          }}
        >
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
      {find.kind !== "closed" && (
        <div
          className="space-y-2 rounded-md border border-border p-2"
          role="search"
          aria-label="Find in query and both bodies"
        >
          <Input
            ref={input}
            aria-label="Find in query, request and response"
            value={find.query}
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
                ? `${find.search.active ? find.search.active.ordinal + 1 : 0} of ${find.search.total} matches · Query parameters ${find.search.queryParameters} · Request body ${state.documents.request.kind === "text" ? find.search.request : "not searchable"} · Response body ${state.documents.response.kind === "text" ? find.search.response : "not searchable"}`
                : find.kind === "searching"
                  ? "Searching complete query and bodies…"
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
            <Button
              onClick={() => {
                findRevision.current += 1;
                setFind({ kind: "closed" });
                findButton.current?.focus();
              }}
            >
              Close Find
            </Button>
          </div>
        </div>
      )}
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
                setMode(value);
                setOffset(0);
                if (find.kind !== "closed")
                  void search(find.query, {
                    representation: value,
                    preserveSide: side,
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
