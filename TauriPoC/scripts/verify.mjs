import { spawnSync } from "node:child_process";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = fileURLToPath(new URL("..", import.meta.url));
function run(command, args, options = {}) {
  const result = spawnSync(command, args, {
    cwd: root,
    stdio: "inherit",
    ...options,
  });
  if (result.status !== 0) process.exit(result.status ?? 1);
  return result;
}
run("cargo", [
  "test",
  "-p",
  "capture-poc-core",
  "--locked",
  "--",
  "--test-threads=1",
]);
run("cargo", ["build", "-p", "capture-poc-core", "--bins", "--locked"]);
const directory = mkdtempSync(
  path.join(tmpdir(), "mobile-capture-poc-verify-"),
);
try {
  const result = run(
    path.join(
      root,
      `target/debug/fixture-export${process.platform === "win32" ? ".exe" : ""}`,
    ),
    [directory],
    { encoding: "utf8", stdio: ["ignore", "pipe", "inherit"] },
  );
  run(process.platform === "win32" ? "python" : "python3", [
    "scripts/verify-evidence.py",
    result.stdout.trim(),
  ]);
} finally {
  rmSync(directory, { recursive: true, force: true });
}
