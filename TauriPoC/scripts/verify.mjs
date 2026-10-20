import { spawnSync } from "node:child_process";

const result = spawnSync("cargo", ["test", "-p", "capture-poc-core", "--", "--test-threads=1"], { stdio: "inherit", cwd: new URL("..", import.meta.url) });
if (result.status !== 0) process.exit(result.status ?? 1);
