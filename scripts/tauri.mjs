import { spawn } from "node:child_process";
import { existsSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { effectiveTauriTargetDir } from "./build-cache.mjs";

const projectRoot = fileURLToPath(new URL("..", import.meta.url));
const binary = join(
  projectRoot,
  "node_modules",
  ".bin",
  process.platform === "win32" ? "tauri.cmd" : "tauri",
);

if (!existsSync(binary)) {
  console.error("未找到 Tauri CLI，请先运行 npm install");
  process.exit(1);
}

const child = spawn(binary, process.argv.slice(2), {
  cwd: projectRoot,
  env: {
    ...process.env,
    CARGO_TARGET_DIR: effectiveTauriTargetDir(),
  },
  shell: process.platform === "win32",
  stdio: "inherit",
});

child.on("error", (error) => {
  console.error(`启动 Tauri CLI 失败：${error.message}`);
  process.exitCode = 1;
});
child.on("exit", (code, signal) => {
  if (signal) {
    console.error(`Tauri CLI 被信号 ${signal} 终止`);
    process.exitCode = 1;
  } else {
    process.exitCode = code ?? 1;
  }
});
