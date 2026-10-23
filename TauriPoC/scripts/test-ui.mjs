import { build } from "esbuild";
import { mkdtemp, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { pathToFileURL, fileURLToPath } from "node:url";
const directory = await mkdtemp(join(tmpdir(), "mobile-capture-ui-"));
try {
  const output = join(directory, "ui-checks.cjs");
  await build({
    entryPoints: [
      fileURLToPath(new URL("../tests/ui-behavior.tsx", import.meta.url)),
    ],
    outfile: output,
    bundle: true,
    platform: "node",
    format: "cjs",
    jsx: "automatic",
  });
  await import(pathToFileURL(output).href);
  await import(new URL("./test-reading.mjs", import.meta.url));
} finally {
  await rm(directory, { recursive: true, force: true });
}
