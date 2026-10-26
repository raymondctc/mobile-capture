import assert from "node:assert/strict";
import { bodyDocumentsSchema, type BodyDocuments } from "../src/product";
import {
  bodyText,
  bodyWindow,
  formatJson,
  initialSide,
  prepareDocuments,
  searchBodies,
  visibleMatches,
  parseQuery,
  queryWindow,
  headerWindow,
} from "../src/body-document";

function fixture(pair: {
  request: Record<string, unknown>;
  response: Record<string, unknown>;
}): BodyDocuments {
  const enrich = (body: Record<string, unknown>) => ({
    retained:
      body.kind === "unavailable"
        ? { kind: "unavailable", reason: "missing" }
        : { kind: "available", bytes: body.bytes },
    media: {
      content_type: null,
      content_encoding: null,
      label: "Unspecified media type",
    },
    basis: "retained",
    ...body,
  });
  return bodyDocumentsSchema.parse({
    reference: { session_key: "/synthetic", event_id: "one" },
    request_info: { method: "GET", url: null },
    request_headers: { kind: "absent" },
    response_headers: { kind: "absent" },
    request: enrich(pair.request),
    response: enrich(pair.response),
  });
}
const original =
  '{"10":9007199254740993,"2":-0,"a":1e+09,"a":"\\u0061\\/\\\\\\\"","nested":[true,null,{}]}';
const formatted = formatJson(original);
assert.equal(formatted.kind, "ready");
if (formatted.kind === "ready") {
  assert.equal(
    formatted.text,
    '{\n  "10": 9007199254740993,\n  "2": -0,\n  "a": 1e+09,\n  "a": "\\u0061\\/\\\\\\\"",\n  "nested": [\n    true,\n    null,\n    {}\n  ]\n}',
  );
}
assert.equal(formatJson("{invalid}").kind, "unavailable");
assert.equal(
  formatJson("[".repeat(400_000) + "0" + "]".repeat(400_000)).kind,
  "unavailable",
);
const pair = prepareDocuments(
  fixture({
    request: {
      kind: "text",
      original: "needle ".repeat(2) + "x".repeat(90_000) + "NeEdLe",
      bytes: 90020,
      basis: "retained",
    },
    response: {
      kind: "text",
      original: "z".repeat(65534) + "needle" + "z".repeat(90000) + "NEEDLE",
      bytes: 155546,
      basis: "decoded",
    },
  }),
);
assert.equal(initialSide(pair), "response");
let search = searchBodies(pair, "original", "needle");
assert.deepEqual([search.request, search.response, search.total], [3, 2, 5]);
assert.deepEqual(search.active, {
  region: { kind: "body", side: "request" },
  start: 0,
  end: 6,
  ordinal: 0,
});
search = searchBodies(pair, "original", "needle", 4);
assert.equal(
  search.active?.region.kind === "body" && search.active.region.side,
  "response",
);
assert.equal(search.active?.ordinal, 4);
search = searchBodies(pair, "original", "needle", 0);
assert.equal(
  search.active?.region.kind === "body" && search.active.region.side,
  "request",
);
assert.equal(search.active?.ordinal, 0);
search = searchBodies(pair, "original", "needle", 2);
assert.equal(search.active?.start, 90014);
search = searchBodies(pair, "original", "needle", 3);
assert.equal(
  search.active?.region.kind === "body" && search.active.region.side,
  "response",
);
assert.equal(search.active?.start, 65534);
const text = bodyText(pair.response, "original")!;
const window = bodyWindow(text, search.active!.start - 100);
assert.ok(window.text.includes("needle"));
assert.ok(window.text.length < 20000);
assert.ok(window.total > 150000);
assert.deepEqual(visibleMatches(text, "needle", 65536, 65550), [
  { start: 65536, end: 65540 },
]);
const flood = prepareDocuments(
  fixture({
    request: {
      kind: "text",
      original: "x".repeat(200000),
      bytes: 200000,
      basis: "retained",
    },
    response: { kind: "text", original: "", bytes: 0, basis: "retained" },
  }),
);
const flooded = searchBodies(flood, "original", "X");
assert.equal(flooded.total, 200000);
assert.equal(
  searchBodies(flood, "original", "x", 199999).active?.start,
  199999,
);
assert.equal(initialSide(flood), "response");
const noResponse = prepareDocuments(
  fixture({
    request: { kind: "binary", bytes: 2, reason: "binary" },
    response: {
      kind: "unavailable",
      reason: "missing",
    },
  }),
);
assert.equal(initialSide(noResponse), "request");
assert.equal(bodyText(noResponse.request, "original"), null);
const unicode = "😀".repeat(20000);
const unicodeWindow = bodyWindow(unicode, 1);
assert.equal(unicodeWindow.start, 0);
assert.equal(unicodeWindow.text.includes("\uFFFD"), false);
const literal = prepareDocuments(
  fixture({
    request: {
      kind: "text",
      original: "[a.*] [A.*]",
      bytes: 13,
      basis: "retained",
    },
    response: {
      kind: "unavailable",
      reason: "missing",
    },
  }),
);
assert.equal(searchBodies(literal, "original", "[a.*]").total, 2);
console.log(
  "Body document checks passed. Lexical JSON fidelity, full two-body Find, reverse and wrap, tail and cross-window matches, match floods, bounded long lines, and empty/binary/missing states.",
);

const manyLines = "x\n".repeat(20000);
const firstLines = bodyWindow(manyLines, 0);
assert.ok(firstLines.text.split("\n").length <= 257);
const secondLines = bodyWindow(manyLines, firstLines.end);
const thirdLines = bodyWindow(manyLines, secondLines.end);
assert.equal(secondLines.previous, firstLines.start);
assert.equal(thirdLines.previous, secondLines.start);

const query = parseQuery(
  "https://example.invalid/path?a+b=a%2Bb&same=one&same=two&empty=&flag&&=v&bad=%C3%28+&percent=%ZZ#ignored?x=y",
);
assert.equal(query.kind, "present");
if (query.kind === "present") {
  assert.equal(
    query.raw,
    "a+b=a%2Bb&same=one&same=two&empty=&flag&&=v&bad=%C3%28+&percent=%ZZ",
  );
  assert.deepEqual(
    query.rows.map((row) => [row.key.text, row.value.text, row.hasEquals]),
    [
      ["a b", "a+b", true],
      ["same", "one", true],
      ["same", "two", true],
      ["empty", "", true],
      ["flag", "", false],
      ["", "", false],
      ["", "v", true],
      ["bad", "%C3%28+", true],
      ["percent", "%ZZ", true],
    ],
  );
  assert.equal(query.rows[7].value.decoding, "literal-invalid-encoding");
  assert.equal(query.rows[8].value.decoding, "literal-invalid-encoding");
}
assert.deepEqual(parseQuery("https://example.invalid/?#fragment"), {
  kind: "present",
  raw: "",
  rows: [],
});
assert.deepEqual(parseQuery("https://example.invalid/#fragment?not=query"), {
  kind: "absent",
});
assert.deepEqual(parseQuery(null), { kind: "unavailable" });
const manyRows = Array.from(
  { length: 120 },
  (_, index) =>
    `k${index}=${index === 119 ? "x".repeat(100_000) + "needle" : "value"}`,
).join("&");
const queryPair = prepareDocuments({
  ...fixture({
    request: { kind: "text", original: "needle", bytes: 6 },
    response: { kind: "text", original: "needle", bytes: 6 },
  }),
  request_info: { method: "PUT", url: `https://example.invalid/?${manyRows}` },
});
const ordered = searchBodies(queryPair, "original", "needle", 0);
assert.deepEqual(
  [ordered.queryParameters, ordered.request, ordered.response, ordered.total],
  [1, 1, 1, 3],
);
assert.deepEqual(ordered.active, {
  region: { kind: "query", row: 119, field: "value" },
  start: 100_000,
  end: 100_006,
  ordinal: 0,
});
assert.deepEqual(
  searchBodies(queryPair, "original", "needle", 1).active?.region,
  { kind: "body", side: "request" },
);
assert.deepEqual(
  searchBodies(queryPair, "original", "needle", 2).active?.region,
  { kind: "body", side: "response" },
);
assert.equal(searchBodies(queryPair, "original", "valuek").total, 0);
const queryPage = queryWindow(queryPair.query, 100, "needle");
assert.equal(queryPage?.total, 120);
assert.equal(queryPage?.rows.length, 20);
assert.equal(queryPage?.rows[19].value.text.length, 256);
assert.equal(queryPage?.rows[19].value.total, 100_006);
assert.equal(queryWindow(queryPair.query, 0)?.rows.length, 50);
console.log(
  "Query document checks passed. Exact raw extraction, form decoding, malformed UTF-8 diagnostics, ordered duplicates and empty segments, complete tail Find, region boundaries, row and cell bounds.",
);

const headers = prepareDocuments({
  ...fixture({
    request: { kind: "text", original: "needle", bytes: 6 },
    response: { kind: "text", original: "needle", bytes: 6 },
  }),
  request_headers: {
    kind: "present",
    rows: Array.from({ length: 120 }, (_, row) => ({
      name: row === 0 ? "NeEdLe" : "X-Duplicate",
      value: row === 119 ? "x".repeat(100_000) + "needle" : "same",
    })),
  },
  response_headers: {
    kind: "present",
    rows: [{ name: "needle", value: "needle" }],
  },
  request_info: { method: "GET", url: "https://example.invalid/?q=needle" },
});
assert.deepEqual(
  [0, 1, 2, 3, 4, 5, 6].map(
    (ordinal) =>
      searchBodies(headers, "original", "needle", ordinal).active?.region,
  ),
  [
    { kind: "header", side: "request", row: 0, field: "name" },
    { kind: "header", side: "request", row: 119, field: "value" },
    { kind: "query", row: 0, field: "value" },
    { kind: "body", side: "request" },
    { kind: "header", side: "response", row: 0, field: "name" },
    { kind: "header", side: "response", row: 0, field: "value" },
    { kind: "body", side: "response" },
  ],
);
const headerSearch = searchBodies(headers, "original", "needle", -1);
assert.deepEqual(
  [
    headerSearch.requestHeaders,
    headerSearch.responseHeaders,
    headerSearch.total,
    headerSearch.active,
  ],
  [2, 2, 7, null],
);
assert.equal(
  headerWindow(headers.headers.request, 100)?.rows[19].value.total,
  100_006,
);
assert.equal(
  headerWindow(headers.headers.request, 100)?.rows[19].value.text.length,
  256,
);
assert.equal(headerWindow(headers.headers.request, 0)?.rows.length, 50);
assert.equal(headerWindow({ kind: "absent" }, 0), null);
assert.equal(
  headerWindow({ kind: "unavailable", reason: "malformed" }, 0),
  null,
);
assert.equal(headerWindow({ kind: "present", rows: [] }, 0)?.total, 0);
console.log(
  "Header document checks passed. Ordered complete names and values, duplicate rows, five-region Find order, null active recomputation and bounded rows/cells.",
);
