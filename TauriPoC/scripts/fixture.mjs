import { spawn, spawnSync } from "node:child_process";
import { mkdtempSync, mkdirSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = fileURLToPath(new URL("..", import.meta.url));
const build = spawnSync(
  "cargo",
  ["build", "-p", "capture-poc-core", "--bins", "--locked"],
  { cwd: root, stdio: "inherit" },
);
if (build.status !== 0) process.exit(build.status ?? 1);
const suffix = process.platform === "win32" ? ".exe" : "";
const binary = process.argv[2];
const fixtureRoot = mkdtempSync(path.join(tmpdir(), "mobile-capture-poc-fixture-"));
const runtimeRoot = path.join(fixtureRoot, "runtime");
mkdirSync(runtimeRoot, { mode: 0o700 });
const env = {
  ...process.env,
  MOBILE_CAPTURE_POC_FIXTURE: "1",
  MOBILE_CAPTURE_POC_FIXTURE_ADB: path.join(
    root,
    `target/debug/fake-adb${suffix}`,
  ),
  MOBILE_CAPTURE_POC_WORKER: binary
    ? path.join(path.dirname(binary), `collector-worker${suffix}`)
    : path.join(root, `target/debug/collector-worker${suffix}`),
  MOBILE_CAPTURE_POC_GUARDIAN: binary
    ? path.join(path.dirname(binary), `capture-guardian${suffix}`)
    : path.join(root, `target/debug/capture-guardian${suffix}`),
  MOBILE_CAPTURE_POC_FIXTURE_ROOT: path.join(fixtureRoot, "sessions"),
  MOBILE_CAPTURE_POC_FIXTURE_RUNTIME_ROOT: runtimeRoot,
};
mkdirSync(env.MOBILE_CAPTURE_POC_FIXTURE_ROOT, { mode: 0o700 });
console.info(
  `FIXTURE MODE. No phone is used. Evidence root: ${env.MOBILE_CAPTURE_POC_FIXTURE_ROOT}`,
);
const command =
  binary ??
  path.join(
    root,
    `node_modules/.bin/tauri${process.platform === "win32" ? ".cmd" : ""}`,
  );
const child = spawn(command, binary ? [] : ["dev"], {
  cwd: root,
  env,
  stdio: "inherit",
  shell: !binary && process.platform === "win32",
});
child.on("exit", (code) => process.exit(code ?? 1));
