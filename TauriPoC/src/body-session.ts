import type { BodyDocument, BodyDocuments, BodySide } from "./product";
import {
  bodyText,
  bodyWindow,
  headerWindow,
  initialSide,
  prepareDocuments,
  queryWindow,
  searchBodies,
  visibleMatches,
  type Documents,
  type QueryTarget,
  type HeaderTarget,
  type Representation,
  type StableMatch,
} from "./body-document";

type TextInfo = Omit<Extract<BodyDocument, { kind: "text" }>, "original"> & {
  formatting: { kind: "ready" } | { kind: "unavailable"; reason: string };
};
export type BodyInfo = TextInfo | Exclude<BodyDocument, { kind: "text" }>;
export type DocumentInfo = Record<BodySide, BodyInfo>;
export type Window = ReturnType<typeof bodyWindow> & {
  matches: ReturnType<typeof visibleMatches>;
};
export type Operation =
  | { kind: "load"; pair: BodyDocuments }
  | {
      kind: "find";
      mode: Representation;
      query: string;
      ordinal: number;
      preferred?: StableMatch;
    }
  | {
      kind: "window";
      side: BodySide;
      mode: Representation;
      query: string;
      offset: number;
    }
  | { kind: "copy"; side: BodySide; mode: Representation }
  | { kind: "query-window"; firstRow: number; query: string }
  | { kind: "header-window"; side: BodySide; firstRow: number; query: string }
  | { kind: "header-text"; target: HeaderTarget; offset: number; query: string }
  | {
      kind: "query-text";
      target: QueryTarget;
      offset: number;
      query: string;
    }
  | {
      kind: "query-copy";
      target: QueryTarget;
    };
export type Reply =
  | {
      kind: "loaded";
      documents: DocumentInfo;
      side: BodySide;
      query: Documents["query"]["kind"];
      headers: Record<
        BodySide,
        | { kind: "present"; total: number }
        | Exclude<Documents["headers"][BodySide], { kind: "present" }>
      >;
    }
  | { kind: "found"; search: ReturnType<typeof searchBodies> }
  | { kind: "window"; window: Window | null }
  | { kind: "copy"; text: string }
  | { kind: "header-window"; window: ReturnType<typeof headerWindow> }
  | { kind: "query-window"; window: ReturnType<typeof queryWindow> };
export type Request = { id: number; operation: Operation };
export type Response = { id: number } & (
  | { kind: "result"; reply: Reply }
  | { kind: "error"; message: string }
);

function info(document: Documents[BodySide]): BodyInfo {
  if (document.kind !== "text") return document;
  return {
    kind: "text",
    retained: document.retained,
    media: document.media,
    bytes: document.bytes,
    basis: document.basis,
    formatting:
      document.formatting.kind === "ready"
        ? { kind: "ready" }
        : document.formatting,
  };
}
export function createSession() {
  let documents: Documents | undefined;
  return (operation: Operation): Reply => {
    if (operation.kind === "load") {
      documents = prepareDocuments(operation.pair);
      return {
        kind: "loaded",
        documents: {
          request: info(documents.request),
          response: info(documents.response),
        },
        side: initialSide(documents),
        query: documents.query.kind,
        headers: {
          request: headerInfo(documents.headers.request),
          response: headerInfo(documents.headers.response),
        },
      };
    }
    if (!documents) throw new Error("Body documents are not loaded");
    switch (operation.kind) {
      case "header-window":
        return {
          kind: "header-window",
          window: headerWindow(
            documents.headers[operation.side],
            operation.firstRow,
            operation.query,
          ),
        };
      case "header-text": {
        const headers = documents.headers[operation.target.side];
        if (headers.kind !== "present")
          throw new Error("Headers are unavailable");
        const text = headers.rows[operation.target.row][operation.target.field];
        const window = bodyWindow(text, operation.offset);
        return {
          kind: "window",
          window: {
            ...window,
            matches: visibleMatches(
              text,
              operation.query,
              window.start,
              window.end,
            ),
          },
        };
      }
      case "query-window":
        return {
          kind: "query-window",
          window: queryWindow(
            documents.query,
            operation.firstRow,
            operation.query,
          ),
        };
      case "query-text":
      case "query-copy": {
        if (documents.query.kind !== "present")
          throw new Error("Query parameters are unavailable");
        const target = operation.target;
        const text =
          target.kind === "raw"
            ? documents.query.raw
            : documents.query.rows[target.row][target.field].text;
        if (operation.kind === "query-copy") return { kind: "copy", text };
        const window = bodyWindow(text, operation.offset);
        return {
          kind: "window",
          window: {
            ...window,
            matches:
              target.kind === "raw"
                ? []
                : visibleMatches(
                    text,
                    operation.query,
                    window.start,
                    window.end,
                  ),
          },
        };
      }
      case "find":
        return {
          kind: "found",
          search: searchBodies(
            documents,
            operation.mode,
            operation.query,
            operation.ordinal,
            operation.preferred,
          ),
        };
      case "window": {
        const text = bodyText(documents[operation.side], operation.mode);
        if (text === null) return { kind: "window", window: null };
        const window = bodyWindow(text, operation.offset);
        return {
          kind: "window",
          window: {
            ...window,
            matches: visibleMatches(
              text,
              operation.query,
              window.start,
              window.end,
            ),
          },
        };
      }
      case "copy": {
        const text = bodyText(documents[operation.side], operation.mode);
        if (text === null) throw new Error("This body has no searchable text");
        return { kind: "copy", text };
      }
      default: {
        const exhaustive: never = operation;
        return exhaustive;
      }
    }
  };
}

function headerInfo(headers: Documents["headers"][BodySide]) {
  return headers.kind === "present"
    ? { kind: headers.kind, total: headers.rows.length }
    : headers;
}
