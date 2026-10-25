import type { BodyDocument, BodyDocuments, BodySide } from "./product";
import {
  bodyText,
  bodyWindow,
  initialSide,
  prepareDocuments,
  searchBodies,
  visibleMatches,
  type Documents,
  type Representation,
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
  | { kind: "find"; mode: Representation; query: string; ordinal: number }
  | {
      kind: "window";
      side: BodySide;
      mode: Representation;
      query: string;
      offset: number;
    }
  | { kind: "copy"; side: BodySide; mode: Representation };
export type Reply =
  | { kind: "loaded"; documents: DocumentInfo; side: BodySide }
  | { kind: "found"; search: ReturnType<typeof searchBodies> }
  | { kind: "window"; window: Window | null }
  | { kind: "copy"; text: string };
export type Request = { id: number; operation: Operation };
export type Response = { id: number } & (
  | { kind: "result"; reply: Reply }
  | { kind: "error"; message: string }
);

function info(document: Documents[BodySide]): BodyInfo {
  if (document.kind !== "text") return document;
  return {
    kind: "text",
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
      };
    }
    if (!documents) throw new Error("Body documents are not loaded");
    switch (operation.kind) {
      case "find":
        return {
          kind: "found",
          search: searchBodies(
            documents,
            operation.mode,
            operation.query,
            operation.ordinal,
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
