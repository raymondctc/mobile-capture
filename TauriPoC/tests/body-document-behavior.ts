import assert from "node:assert/strict";
import { bodyDocumentsSchema } from "../src/product";
import {
  bodyText,
  bodyWindow,
  formatJson,
  initialSide,
  prepareDocuments,
  searchBodies,
  visibleMatches,
} from "../src/body-document";

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
  bodyDocumentsSchema.parse({
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
  side: "request",
  start: 0,
  end: 6,
  ordinal: 0,
});
search = searchBodies(pair, "original", "needle", 4);
assert.equal(search.active?.side, "response");
assert.equal(search.active?.ordinal, 4);
search = searchBodies(pair, "original", "needle", 0);
assert.equal(search.active?.side, "request");
assert.equal(search.active?.ordinal, 0);
search = searchBodies(pair, "original", "needle", 2);
assert.equal(search.active?.start, 90014);
search = searchBodies(pair, "original", "needle", 3);
assert.equal(search.active?.side, "response");
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
  bodyDocumentsSchema.parse({
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
  bodyDocumentsSchema.parse({
    request: { kind: "binary", bytes: 2, reason: "binary" },
    response: {
      kind: "unavailable",
      retained_available: false,
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
  bodyDocumentsSchema.parse({
    request: {
      kind: "text",
      original: "[a.*] [A.*]",
      bytes: 13,
      basis: "retained",
    },
    response: {
      kind: "unavailable",
      retained_available: false,
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
