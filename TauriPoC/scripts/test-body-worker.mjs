import assert from "node:assert/strict";
import { Worker } from "node:worker_threads";
import { mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { build } from "esbuild";
const directory = await mkdtemp(join(tmpdir(), "body-document-worker-"));
let worker;
try {
  await build({
    entryPoints: [
      fileURLToPath(new URL("../src/body-worker.ts", import.meta.url)),
    ],
    outfile: join(directory, "worker.mjs"),
    bundle: true,
    platform: "browser",
    format: "esm",
  });
  await writeFile(
    join(directory, "bootstrap.mjs"),
    `import { parentPort } from 'node:worker_threads';
    globalThis.self = { postMessage: (value) => parentPort.postMessage(value) };
    await import('./worker.mjs');
    parentPort.on('message', (data) => self.onmessage({ data }));`,
  );
  worker = new Worker(join(directory, "bootstrap.mjs"), { type: "module" });
  let sequence = 0;
  const read = (operation) =>
    new Promise((resolve, reject) => {
      const id = ++sequence;
      const reply = (message) => {
        if (message.id !== id) return;
        worker.off("message", reply);
        worker.off("error", reject);
        if (message.kind === "error") reject(new Error(message.message));
        else resolve(message.reply);
      };
      worker.on("message", reply);
      worker.once("error", reject);
      worker.postMessage({ id, operation });
    });
  const text = '{"10":9007199254740993,"2":-0,"a":1e+09,"a":"needle"}';
  const body = (original) => ({
    retained: { kind: "available", bytes: Buffer.byteLength(original) },
    media: {
      content_type: null,
      content_encoding: null,
      label: "Unspecified media type",
    },
    kind: "text",
    original,
    bytes: Buffer.byteLength(original),
    basis: "retained",
  });
  const pair = (request, response, url = null) => ({
    reference: { session_key: "/synthetic", event_id: "one" },
    request_info: { method: "POST", url },
    request_headers: { kind: "absent" },
    response_headers: { kind: "absent" },
    request,
    response,
  });
  const loaded = await read({
    kind: "load",
    pair: pair(body("request needle"), body(text)),
  });
  assert.equal(loaded.side, "response");
  assert.equal(loaded.documents.response.formatting.kind, "ready");
  const found = await read({
    kind: "find",
    mode: "formatted",
    query: "needle",
    ordinal: 1,
  });
  assert.equal(found.search.total, 2);
  assert.equal(found.search.active.region.side, "response");
  const window = await read({
    kind: "window",
    side: "response",
    mode: "formatted",
    query: "needle",
    offset: 0,
  });
  assert.ok(window.window.text.includes('"10": 9007199254740993'));
  assert.equal(window.window.matches.length, 1);
  assert.equal(
    (await read({ kind: "copy", side: "response", mode: "original" })).text,
    text,
  );
  const rawQuery =
    "q=needle&duplicate=one&duplicate=two&flag&empty=&bad=%C3%28+&long=" +
    "x".repeat(100000) +
    "needle";
  const queryLoaded = await read({
    kind: "load",
    pair: pair(
      body("request needle"),
      body(text),
      `https://example.invalid/?${rawQuery}#ignored=needle`,
    ),
  });
  assert.equal(queryLoaded.query, "present");
  assert.equal(queryLoaded.side, "response");
  const queryFound = await read({
    kind: "find",
    mode: "original",
    query: "needle",
    ordinal: 1,
  });
  assert.deepEqual(
    [
      queryFound.search.queryParameters,
      queryFound.search.request,
      queryFound.search.response,
      queryFound.search.total,
    ],
    [2, 1, 1, 4],
  );
  assert.deepEqual(queryFound.search.active.region, {
    kind: "query",
    row: 6,
    field: "value",
  });
  const queryPreserved = await read({
    kind: "find",
    mode: "formatted",
    query: "needle",
    ordinal: 0,
    preferred: {
      region: queryFound.search.active.region,
      start: queryFound.search.active.start,
    },
  });
  assert.deepEqual(queryPreserved.search.active, queryFound.search.active);
  const queryTail = await read({
    kind: "query-text",
    target: { kind: "cell", row: 6, field: "value" },
    query: "needle",
    offset: 99800,
  });
  assert.ok(queryTail.window.text.endsWith("needle"));
  assert.ok(queryTail.window.text.length <= 16384);
  assert.equal(queryTail.window.total, 100006);
  assert.equal(queryTail.window.matches.length, 1);
  assert.equal(
    (await read({ kind: "query-copy", target: { kind: "raw" } })).text,
    rawQuery,
  );
  assert.equal(
    (
      await read({
        kind: "query-copy",
        target: { kind: "cell", row: 6, field: "value" },
      })
    ).text,
    "x".repeat(100000) + "needle",
  );
  const queryPage = await read({
    kind: "query-window",
    firstRow: 0,
    query: "needle",
  });
  assert.equal(queryPage.window.rows[5].value.text, "%C3%28+");
  assert.equal(
    queryPage.window.rows[5].value.decoding,
    "literal-invalid-encoding",
  );
  assert.equal(queryPage.window.rows[6].value.total, 100006);
  assert.equal(queryPage.window.rows[6].value.text.length, 256);
  const headerPair = pair(
    body("needle"),
    body("needle"),
    "https://example.invalid/?q=needle",
  );
  headerPair.request_headers = {
    kind: "present",
    rows: Array.from({ length: 120 }, (_, row) => ({
      name: "X-Duplicate",
      value: row === 119 ? "x".repeat(100000) + "needle" : "same",
    })),
  };
  headerPair.response_headers = {
    kind: "present",
    rows: [{ name: "NEEDLE", value: "needle" }],
  };
  const headerLoaded = await read({ kind: "load", pair: headerPair });
  assert.equal(headerLoaded.headers.request.total, 120);
  const headerFound = await read({
    kind: "find",
    mode: "original",
    query: "needle",
    ordinal: 0,
  });
  assert.deepEqual(
    [
      headerFound.search.requestHeaders,
      headerFound.search.queryParameters,
      headerFound.search.request,
      headerFound.search.responseHeaders,
      headerFound.search.response,
      headerFound.search.total,
    ],
    [1, 1, 1, 2, 1, 6],
  );
  assert.deepEqual(headerFound.search.active.region, {
    kind: "header",
    side: "request",
    row: 119,
    field: "value",
  });
  const headerRows = await read({
    kind: "header-window",
    side: "request",
    firstRow: 100,
    query: "needle",
  });
  assert.equal(headerRows.window.rows.length, 20);
  assert.equal(headerRows.window.rows[19].value.text.length, 256);
  const headerTail = await read({
    kind: "header-text",
    target: headerFound.search.active.region,
    query: "needle",
    offset: 99800,
  });
  assert.equal(headerTail.window.total, 100006);
  assert.ok(headerTail.window.text.endsWith("needle"));
  assert.ok(headerTail.window.text.length <= 16384);
  assert.equal(headerTail.window.matches.length, 1);
  const flood = "x".repeat(32 * 1024 * 1024);
  await read({
    kind: "load",
    pair: pair(body(flood), body(flood)),
  });
  let timerRan = false;
  const pending = read({
    kind: "find",
    mode: "original",
    query: "x",
    ordinal: 67108863,
  });
  const timer = setTimeout(() => {
    timerRan = true;
  }, 20);
  const complete = await pending;
  clearTimeout(timer);
  assert.equal(
    timerRan,
    true,
    "The main thread serves its timer while complete Find runs in the worker",
  );
  assert.equal(complete.search.total, 67108864);
  assert.equal(complete.search.active.start, 33554431);
  const tail = await read({
    kind: "window",
    side: "response",
    mode: "original",
    query: "x",
    offset: 33554000,
  });
  assert.ok(tail.window.text.length < 20000);
  assert.equal(tail.window.end, 33554432);
  assert.equal(tail.window.matches.length, tail.window.text.length);
  await assert.rejects(
    read({ kind: "copy", side: "missing", mode: "original" }),
  );
  console.log(
    "Body module-worker checks passed. Real worker messages preserve lexical text, full-copy values, complete flood counts, bounded tail highlights, errors, and main-thread timer progress.",
  );
} finally {
  if (worker) await worker.terminate();
  await rm(directory, { recursive: true, force: true });
}
