import { existsSync } from "node:fs";
import { join } from "node:path";
import { localCargoEnv, projectRoot } from "./build-cache.mjs";
import { runBuildCommand } from "./build-runner.mjs";

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

const args = process.argv.slice(2);
const separator = args.indexOf("--");
const options = separator < 0 ? args : args.slice(0, separator);
process.exitCode = await runBuildCommand(binary, args, {
  env: localCargoEnv(),
  requiresBuildSpace: options.some((argument) => ["dev", "build"].includes(argument))
    && !options.some((argument) => ["-h", "--help", "-V", "--version"].includes(argument)),
});
