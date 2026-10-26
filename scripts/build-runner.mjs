import { spawn } from "node:child_process";
import { constants } from "node:os";
import { localCargoEnv, projectRoot, reportBuildStorage } from "./build-cache.mjs";

export async function runBuildCommand(command, args, {
  cwd = projectRoot, env = localCargoEnv(), requiresBuildSpace = false,
} = {}) {
  if (requiresBuildSpace && !reportBuildStorage(env, { phase: "构建前", requireFreeSpace: true })) return 1;
  const result = await new Promise((resolve) => {
    const child = spawn(command, args, {
      cwd, env, stdio: "inherit",
      shell: process.platform === "win32" && /\.(cmd|bat)$/i.test(command),
    });
    child.once("error", (error) => resolve({ error }));
    child.once("close", (code, signal) => resolve({ code, signal }));
  });
  const storageHealthy = !requiresBuildSpace || reportBuildStorage(env, { phase: "构建后", requireFreeSpace: true });
  if (result.error) {
    console.error(`无法执行 ${command}：${result.error.message}`);
    return 1;
  }
  if (result.signal) return 128 + (constants.signals[result.signal] || 1);
  return result.code || (storageHealthy ? 0 : 1);
}
