import { createRequire } from "node:module";
import { mkdtemp, readFile, rm } from "node:fs/promises";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { build } from "esbuild";

const require = createRequire(import.meta.url);
const { JSDOM } = require("jsdom");
const dom = new JSDOM("<!doctype html><html><body></body></html>", {
  url: "http://localhost/",
  pretendToBeVisual: true,
});
for (const name of [
  "window",
  "document",
  "navigator",
  "HTMLElement",
  "HTMLInputElement",
  "HTMLButtonElement",
  "HTMLSelectElement",
  "Node",
  "NodeFilter",
  "Element",
  "DocumentFragment",
  "MutationObserver",
  "CustomEvent",
  "Event",
  "KeyboardEvent",
  "MouseEvent",
  "FocusEvent",
  "getComputedStyle",
  "localStorage",
])
  Object.defineProperty(globalThis, name, {
    configurable: true,
    value:
      name === "getComputedStyle"
        ? dom.window.getComputedStyle.bind(dom.window)
        : dom.window[name],
  });
globalThis.PointerEvent = dom.window.MouseEvent;
globalThis.requestAnimationFrame = dom.window.requestAnimationFrame.bind(
  dom.window,
);
globalThis.cancelAnimationFrame = dom.window.cancelAnimationFrame.bind(
  dom.window,
);
globalThis.IS_REACT_ACT_ENVIRONMENT = true;
const directory = await mkdtemp(
  fileURLToPath(
    new URL("../node_modules/.android-dom-checks-", import.meta.url),
  ),
);
try {
  const output = join(directory, "checks.cjs");
  await build({
    entryPoints: [
      fileURLToPath(new URL("../tests/reading-behavior.tsx", import.meta.url)),
    ],
    outfile: output,
    bundle: true,
    platform: "node",
    format: "cjs",
    jsx: "automatic",
    external: [
      "react",
      "react-dom",
      "react-dom/*",
      "@testing-library/*",
      "radix-ui",
      "lucide-react",
      "zod",
      "cn",
      "class-variance-authority",
      "@tauri-apps/api/*",
    ],
    plugins: [
      {
        name: "component-entry",
        setup(build) {
          build.onLoad({ filter: /src\/main\.tsx$/ }, async ({ path }) => {
            const source = await readFile(path, "utf8");
            return {
              contents: source.slice(
                0,
                source.lastIndexOf(
                  'const root = document.getElementById("root");',
                ),
              ),
              loader: "tsx",
            };
          });
        },
      },
    ],
    loader: { ".css": "empty", ".png": "file" },
  });
  await require(output).complete;
} finally {
  dom.window.close();
  await rm(directory, { recursive: true, force: true });
}
