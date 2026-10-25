import { useEffect, useRef, useState } from "react";
import { Button } from "./components/ui/button";
import { Input } from "./components/ui/input";
import { BodyWindow } from "./BodyViewport";
import { type Representation, type Search } from "./body-document";
import { bodyDocuments, type DocumentOwner } from "./body-owner";
import { type DocumentInfo, type Window } from "./body-session";
import { errorMessage } from "./api";
import { product, type BodySide } from "./product";

type State =
  | { kind: "loading" }
  | { kind: "ready"; documents: DocumentInfo }
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
const emptySearch: Search = { request: 0, response: 0, total: 0, active: null };
const sides: BodySide[] = ["request", "response"];
export function BodyInspector({
  id,
  wrap,
  visible,
  paneVisible,
  onFindOpen,
  onRaw,
}: {
  id: string;
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
    let document: DocumentOwner;
    try {
      document = bodyDocuments.create();
    } catch (cause) {
      setState({ kind: "error", message: errorMessage(cause) });
      return;
    }
    owner.current = document;
    void product
      .bodyDocuments(id)
      .then((pair) => document.read({ kind: "load", pair }))
      .then((reply) => {
        if (request !== revision.current) return;
        if (reply.kind !== "loaded")
          throw new Error("Unexpected body document reply");
        setSide(reply.side);
        setState({ kind: "ready", documents: reply.documents });
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
  }, [id]);
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
      setSide(search.active.side);
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
    const request = ++findRevision.current;
    setFind({ kind: "searching", query });
    try {
      if (debounce) await new Promise((resolve) => setTimeout(resolve, 150));
      if (request !== findRevision.current || !owner.current) return;
      let reply = await owner.current.read({
        kind: "find",
        mode: representation,
        query,
        ordinal,
      });
      if (request !== findRevision.current || !owner.current) return;
      if (reply.kind !== "found") throw new Error("Unexpected body Find reply");
      if (preserveSide && reply.search.active?.side !== preserveSide) {
        if (reply.search[preserveSide]) {
          reply = await owner.current.read({
            kind: "find",
            mode: representation,
            query,
            ordinal: preserveSide === "response" ? reply.search.request : 0,
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
    const ordinal = (current + direction + find.search.total) % find.search.total;
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
  return (
    <div className="space-y-3">
      <div
        role="tablist"
        aria-label="Network bodies"
        className="flex flex-wrap gap-2"
      >
        {sides.map((value) => (
          <Button
            key={value}
            role="tab"
            variant={side === value ? "default" : "outline"}
            aria-selected={side === value}
            onClick={() => {
              setSide(value);
              setOffset(0);
              setCopy({ kind: "idle" });
            }}
          >
            {value === "request" ? "Request" : "Response"}
            {find.kind !== "closed" && find.query
              ? ` (${state.documents[value].kind === "text" ? (find.kind === "complete" ? find.search[value] : "…") : "not searchable"})`
              : ""}
          </Button>
        ))}
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
          aria-label="Find in both bodies"
        >
          <Input
            ref={input}
            aria-label="Find in request and response"
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
                ? `${find.search.active ? find.search.active.ordinal + 1 : 0} of ${find.search.total} matches · Request ${state.documents.request.kind === "text" ? find.search.request : "not searchable"} · Response ${state.documents.response.kind === "text" ? find.search.response : "not searchable"}`
                : find.kind === "searching"
                  ? "Searching complete bodies…"
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
      {body.kind === "text" && (
        <>
          <div className="flex flex-wrap gap-2">
            <Button
              variant={
                mode === "original" || body.formatting.kind === "unavailable"
                  ? "default"
                  : "outline"
              }
              aria-pressed={
                mode === "original" || body.formatting.kind === "unavailable"
              }
              onClick={() => {
                setMode("original");
                setOffset(0);
                if (find.kind !== "closed")
                  void search(find.query, {
                    representation: "original",
                    preserveSide: side,
                  });
              }}
            >
              Original
            </Button>
            <Button
              variant={
                mode === "formatted" && body.formatting.kind === "ready"
                  ? "default"
                  : "outline"
              }
              aria-pressed={
                mode === "formatted" && body.formatting.kind === "ready"
              }
              disabled={body.formatting.kind !== "ready"}
              onClick={() => {
                setMode("formatted");
                setOffset(0);
                if (find.kind !== "closed")
                  void search(find.query, {
                    representation: "formatted",
                    preserveSide: side,
                  });
              }}
            >
              Formatted
            </Button>
            <Button
              disabled={copy.kind === "pending"}
              onClick={() => void copyBody("original")}
            >
              Copy original
            </Button>
            <Button
              disabled={
                copy.kind === "pending" || body.formatting.kind !== "ready"
              }
              onClick={() => void copyBody("formatted")}
            >
              Copy formatted
            </Button>
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
      {body.kind !== "text" && (
        <p role="status">
          {body.reason} Text Find and copy are unavailable for this body.
        </p>
      )}
      {copy.kind === "error" && <p role="alert">Copy failed. {copy.message}</p>}
      {copy.kind === "done" && (
        <p role="status">Copied complete {copy.mode} body.</p>
      )}
      {window.kind === "loading" && body.kind === "text" && (
        <p role="status">Reading body window…</p>
      )}
      {window.kind === "error" && <p role="alert">{window.message}</p>}
      {window.kind === "ready" && window.value && (
        <BodyWindow
          window={window.value}
          onOffset={setOffset}
          wrap={wrap}
          active={
            find.kind === "complete" && find.search.active?.side === side
              ? find.search.active
              : null
          }
        />
      )}

      <Button variant="ghost" onClick={() => onRaw(side)}>
        Retained bytes · hexadecimal
      </Button>
    </div>
  );
}
