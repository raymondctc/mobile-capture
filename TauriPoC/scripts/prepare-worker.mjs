import { spawnSync } from "node:child_process";
import { copyFileSync, mkdirSync } from "node:fs";
import { fileURLToPath } from "node:url";
import path from "node:path";

const root = fileURLToPath(new URL("..", import.meta.url));
const info = spawnSync("rustc", ["-vV"], { encoding: "utf8" });
if (info.status !== 0) throw new Error(info.stderr);
const target = info.stdout.match(/^host: (.+)$/m)?.[1];
if (!target) throw new Error("rustc did not report a host target");
const result = spawnSync("cargo", ["build", "-p", "capture-poc-core", "--bin", "collector-worker", "--release", "--locked"], { cwd: root, stdio: "inherit" });
if (result.status !== 0) process.exit(result.status ?? 1);
const suffix = process.platform === "win32" ? ".exe" : "";
mkdirSync(path.join(root, "src-tauri/binaries"), { recursive: true });
copyFileSync(path.join(root, `target/release/collector-worker${suffix}`), path.join(root, `src-tauri/binaries/collector-worker-${target}${suffix}`));
