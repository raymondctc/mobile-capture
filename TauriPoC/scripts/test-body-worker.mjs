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
    kind: "text",
    original,
    bytes: Buffer.byteLength(original),
    basis: "retained",
  });
  const loaded = await read({
    kind: "load",
    pair: { request: body("request needle"), response: body(text) },
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
  assert.equal(found.search.active.side, "response");
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
  const flood = "x".repeat(32 * 1024 * 1024);
  await read({
    kind: "load",
    pair: { request: body(flood), response: body(flood) },
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
