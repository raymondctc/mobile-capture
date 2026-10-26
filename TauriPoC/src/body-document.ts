import type {
  BodyDocument,
  BodyDocuments,
  BodySide,
  HeaderDocument,
} from "./product";

export type Representation = "original" | "formatted";
type Formatting =
  | { kind: "ready"; text: string }
  | { kind: "unavailable"; reason: string };
export type TextDocument = Extract<BodyDocument, { kind: "text" }> & {
  formatting: Formatting;
};
export type Document = TextDocument | Exclude<BodyDocument, { kind: "text" }>;
export type QueryCell = {
  text: string;
  decoding: "decoded" | "literal-invalid-encoding";
};
export type QueryRow = { key: QueryCell; value: QueryCell; hasEquals: boolean };
export type QueryDocument =
  | { kind: "present"; raw: string; rows: QueryRow[] }
  | { kind: "absent" }
  | { kind: "unavailable" };
export type Documents = Record<BodySide, Document> & {
  query: QueryDocument;
  headers: Record<BodySide, HeaderDocument>;
};
export type HeaderTarget = {
  side: BodySide;
  row: number;
  field: "name" | "value";
};
export type QueryTarget =
  | { kind: "raw" }
  | { kind: "cell"; row: number; field: "key" | "value" };
export type Region =
  | ({ kind: "header" } & HeaderTarget)
  | { kind: "query"; row: number; field: "key" | "value" }
  | { kind: "body"; side: BodySide };
export type Match = {
  region: Region;
  start: number;
  end: number;
  ordinal: number;
};
export type Search = {
  requestHeaders: number;
  responseHeaders: number;
  queryParameters: number;
  request: number;
  response: number;
  total: number;
  active: Match | null;
};
export const windowCharacters = 16 * 1024;
const formattedByteBound = 64 * 1024 * 1024;
const sides: BodySide[] = ["request", "response"];

export function formatJson(
  original: string,
  bytes = new TextEncoder().encode(original).length,
): Formatting {
  try {
    JSON.parse(original);
  } catch {
    return {
      kind: "unavailable",
      reason: "Not valid JSON. Showing original text.",
    };
  }
  const chunks: string[] = [];
  let buffer = "";
  const utf8Overhead = bytes - original.length;
  let formattedBytes = utf8Overhead;
  let depth = 0;
  let previous = "";
  const append = (text: string) => {
    formattedBytes += text.length;
    if (formattedBytes > formattedByteBound) return false;
    buffer += text;
    if (buffer.length >= 32768) {
      chunks.push(buffer);
      buffer = "";
    }
    return true;
  };
  for (let index = 0; index < original.length; index += 1) {
    const character = original[index];
    if (
      character === "\t" ||
      character === "\n" ||
      character === "\r" ||
      character === " "
    )
      continue;
    if (previous === "{" || previous === "[" || previous === ",") {
      if (character !== "}" && character !== "]") {
        if (!append(`\n${"  ".repeat(Math.min(depth, 128))}`))
          return formatOverflow();
      }
    }
    if (character === '"') {
      let end = original.indexOf('"', index + 1);
      while (end !== -1) {
        let backslashes = 0;
        for (let cursor = end - 1; original[cursor] === "\\"; cursor -= 1)
          backslashes += 1;
        if (backslashes % 2 === 0) break;
        end = original.indexOf('"', end + 1);
      }
      if (!append(original.slice(index, end + 1))) return formatOverflow();
      index = end;
      previous = '"';
      continue;
    }
    if (character === "}" || character === "]") {
      depth -= 1;
      if (previous !== "{" && previous !== "[") {
        if (!append(`\n${"  ".repeat(Math.min(depth, 128))}`))
          return formatOverflow();
      }
    }
    if (!append(character === ":" ? ": " : character)) return formatOverflow();
    if (character === "{" || character === "[") depth += 1;
    previous = character;
  }
  chunks.push(buffer);
  return { kind: "ready", text: chunks.join("") };
}
function formatOverflow(): Formatting {
  return {
    kind: "unavailable",
    reason:
      "Formatted body exceeds 64 MiB. Showing the complete original text.",
  };
}

export function prepareDocuments(pair: BodyDocuments): Documents {
  const prepare = (body: BodyDocument): Document =>
    body.kind === "text"
      ? { ...body, formatting: formatJson(body.original, body.bytes) }
      : body;
  return {
    request: prepare(pair.request),
    response: prepare(pair.response),
    query: parseQuery(pair.request_info.url),
    headers: { request: pair.request_headers, response: pair.response_headers },
  };
}
export function initialSide(pair: Documents): BodySide {
  return pair.response.kind !== "unavailable" ||
    pair.response.retained.kind === "available"
    ? "response"
    : "request";
}
export function bodyText(body: Document, mode: Representation): string | null {
  if (body.kind !== "text") return null;
  return mode === "formatted" && body.formatting.kind === "ready"
    ? body.formatting.text
    : body.original;
}
function matcher(query: string): RegExp | null {
  return query
    ? new RegExp(query.replace(/[.*+?^${}()|[\]\\]/g, "\\$&"), "giu")
    : null;
}
export function parseQuery(url: string | null): QueryDocument {
  if (url === null) return { kind: "unavailable" };
  const fragment = url.indexOf("#");
  const beforeFragment = fragment === -1 ? url : url.slice(0, fragment);
  const question = beforeFragment.indexOf("?");
  if (question === -1) return { kind: "absent" };
  const raw = beforeFragment.slice(question + 1);
  const decode = (text: string): QueryCell => {
    try {
      return {
        text: decodeURIComponent(text.replace(/\+/g, " ")),
        decoding: "decoded",
      };
    } catch {
      return { text, decoding: "literal-invalid-encoding" };
    }
  };
  return {
    kind: "present",
    raw,
    rows:
      raw === ""
        ? []
        : raw.split("&").map((segment) => {
            const equals = segment.indexOf("=");
            return {
              key: decode(equals === -1 ? segment : segment.slice(0, equals)),
              value: decode(equals === -1 ? "" : segment.slice(equals + 1)),
              hasEquals: equals !== -1,
            };
          }),
  };
}
export function searchBodies(
  pair: Documents,
  mode: Representation,
  query: string,
  ordinal = 0,
): Search {
  const counts = {
    requestHeaders: 0,
    responseHeaders: 0,
    queryParameters: 0,
    request: 0,
    response: 0,
  };
  let total = 0;
  let active: Match | null = null;
  const pattern = matcher(query);
  if (!pattern) return { ...counts, total, active };
  const scan = (text: string, region: Region, source: keyof typeof counts) => {
    pattern.lastIndex = 0;
    let match: RegExpExecArray | null;
    while ((match = pattern.exec(text))) {
      if (total === ordinal)
        active = {
          region,
          start: match.index,
          end: match.index + match[0].length,
          ordinal,
        };
      total += 1;
      counts[source] += 1;
    }
  };
  const scanHeaders = (side: BodySide) => {
    const headers = pair.headers[side];
    if (headers.kind !== "present") return;
    headers.rows.forEach((row, index) => {
      for (const field of ["name", "value"] as const)
        scan(
          row[field],
          { kind: "header", side, row: index, field },
          side === "request" ? "requestHeaders" : "responseHeaders",
        );
    });
  };
  scanHeaders("request");
  if (pair.query.kind === "present")
    pair.query.rows.forEach((row, index) => {
      scan(
        row.key.text,
        { kind: "query", row: index, field: "key" },
        "queryParameters",
      );
      scan(
        row.value.text,
        { kind: "query", row: index, field: "value" },
        "queryParameters",
      );
    });
  for (const side of sides) {
    if (side === "response") scanHeaders(side);
    const text = bodyText(pair[side], mode);
    if (text !== null) scan(text, { kind: "body", side }, side);
  }
  return { ...counts, total, active };
}
export function headerWindow(
  headers: HeaderDocument,
  firstRow: number,
  search = "",
) {
  if (headers.kind !== "present") return null;
  const start = Math.max(
    0,
    Math.min(firstRow, Math.max(0, headers.rows.length - 1)),
  );
  const preview = (text: string) => {
    const end = /[\uDC00-\uDFFF]/.test(text[256] ?? "") ? 255 : 256;
    return {
      text: text.slice(0, end),
      total: text.length,
      matches: visibleMatches(text, search, 0, end),
    };
  };
  return {
    start,
    total: headers.rows.length,
    rows: headers.rows
      .slice(start, start + 50)
      .map((row) => ({ name: preview(row.name), value: preview(row.value) })),
  };
}
export function queryWindow(
  query: QueryDocument,
  firstRow: number,
  search = "",
) {
  if (query.kind !== "present") return null;
  const start = Math.max(
    0,
    Math.min(firstRow, Math.max(0, query.rows.length - 1)),
  );
  const preview = (cell: QueryCell) => {
    const end = /[\uDC00-\uDFFF]/.test(cell.text[256] ?? "") ? 255 : 256;
    const text = cell.text.slice(0, end);
    return {
      ...cell,
      text,
      total: cell.text.length,
      matches: visibleMatches(cell.text, search, 0, text.length),
    };
  };
  return {
    start,
    total: query.rows.length,
    rawCharacters: query.raw.length,
    rows: query.rows.slice(start, start + 50).map((row) => ({
      key: preview(row.key),
      value: preview(row.value),
      hasEquals: row.hasEquals,
    })),
  };
}
export function bodyWindow(text: string, offset: number) {
  let start = Math.max(0, Math.min(offset, Math.max(0, text.length - 1)));
  if (start > 0 && /[\uDC00-\uDFFF]/.test(text[start])) start -= 1;
  let end = Math.min(text.length, start + windowCharacters);
  if (end < text.length && /[\uDC00-\uDFFF]/.test(text[end])) end -= 1;
  let lineEnd = start;
  for (let lines = 0; lines < 256; lines += 1) {
    const newline = text.indexOf("\n", lineEnd);
    if (newline === -1 || newline >= end) break;
    lineEnd = newline + 1;
    if (lines === 255) end = lineEnd;
  }
  let previous = Math.max(0, start - windowCharacters);
  let lines = 0;
  for (let index = start - 1; index >= previous; index -= 1) {
    if (text[index] === "\n" && ++lines > 256) {
      previous = index + 1;
      break;
    }
  }
  return {
    start,
    end,
    previous,
    text: text.slice(start, end),
    total: text.length,
  };
}
export function visibleMatches(
  text: string,
  query: string,
  start: number,
  end: number,
): { start: number; end: number }[] {
  const pattern = matcher(query);
  if (!pattern) return [];
  pattern.lastIndex = 0;
  const result: { start: number; end: number }[] = [];
  let match: RegExpExecArray | null;
  while ((match = pattern.exec(text)) && match.index < end) {
    const matchEnd = match.index + match[0].length;
    if (matchEnd > start)
      result.push({
        start: Math.max(start, match.index),
        end: Math.min(end, matchEnd),
      });
  }
  return result;
}
