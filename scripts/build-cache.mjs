import { execFileSync } from "node:child_process";
import { existsSync, readdirSync, statSync, statfsSync } from "node:fs";
import { homedir } from "node:os";
import { dirname, isAbsolute, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const home = homedir();
const gib = 1024 ** 3;
export const projectRoot = resolve(fileURLToPath(new URL("..", import.meta.url)));

export function defaultTauriTargetDir(environment = process.env) {
  if (process.platform === "darwin") {
    return join(home, "Library", "Caches", "BalanceHub", "cargo-target");
  }
  if (process.platform === "win32") {
    const localAppData = environment.LOCALAPPDATA || join(home, "AppData", "Local");
    return join(localAppData, "BalanceHub", "cargo-target");
  }
  return join(environment.XDG_CACHE_HOME || join(home, ".cache"), "balancehub", "cargo-target");
}

export function effectiveTauriTargetDir(environment = process.env) {
  const override = environment.CARGO_TARGET_DIR?.trim();
  if (!override) return defaultTauriTargetDir(environment);
  return isAbsolute(override) ? override : resolve(projectRoot, override);
}

export function localCargoEnv(environment = process.env) {
  return {
    ...environment,
    CARGO_TARGET_DIR: effectiveTauriTargetDir(environment),
    CARGO_BUILD_JOBS: environment.CARGO_BUILD_JOBS?.trim() || "2",
    CARGO_INCREMENTAL: environment.CARGO_INCREMENTAL?.trim() || "0",
  };
}

export function cargoInvocation(args, environment = process.env) {
  const separator = args.indexOf("--");
  const options = separator < 0 ? args : args.slice(0, separator);
  const env = localCargoEnv(environment);
  const cwd = join(projectRoot, "src-tauri");
  let command;
  const takesValue = new Set(["--color", "--config", "-Z", "-C"]);
  for (let index = 0; index < options.length; index += 1) {
    const argument = options[index];
    if (argument === "--target-dir" && options[index + 1]) {
      env.CARGO_TARGET_DIR = resolve(cwd, options[++index]);
    } else if (argument.startsWith("--target-dir=")) {
      env.CARGO_TARGET_DIR = resolve(cwd, argument.slice("--target-dir=".length));
    } else if (!command && takesValue.has(argument)) {
      index += 1;
    } else if (!command && !argument.startsWith("-") && !argument.startsWith("+")) {
      command = argument;
    }
  }
  const informational = options.some((argument) => ["-h", "--help", "-V", "--version", "--list"].includes(argument));
  const noBuild = new Set(["clean", "fmt", "metadata", "locate-project", "help", "version"]);
  return { env, cwd, requiresBuildSpace: Boolean(command && !informational && !noBuild.has(command)) };
}

function threshold(environment, name, fallback) {
  const raw = environment[name]?.trim();
  const value = raw ? Number(raw) : fallback;
  if (!Number.isFinite(value) || value < 0) throw new Error(`${name} 必须是非负有限数值`);
  return value;
}

function existingDirectory(path) {
  let current = resolve(path);
  while (!existsSync(current) || !statSync(current).isDirectory()) {
    const parent = dirname(current);
    if (parent === current) throw new Error(`无法确定 target 所在磁盘：${path}`);
    current = parent;
  }
  return current;
}

function targetSizeGiB(targetRoot) {
  if (!existsSync(targetRoot)) return 0;
  if (process.platform !== "win32") {
    const output = execFileSync("du", ["-sk", targetRoot], {
      encoding: "utf8", timeout: 30_000, stdio: ["ignore", "pipe", "pipe"],
    });
    const kib = Number.parseInt(output.trim().split(/\s+/)[0], 10);
    if (!Number.isFinite(kib)) throw new Error("du 未返回有效体量");
    return kib / (1024 ** 2);
  }
  let bytes = 0;
  const stack = [targetRoot];
  while (stack.length > 0) {
    const directory = stack.pop();
    for (const entry of readdirSync(directory, { withFileTypes: true })) {
      const path = join(directory, entry.name);
      if (entry.isDirectory()) stack.push(path);
      else if (entry.isFile()) {
        try { bytes += statSync(path).size; }
        catch (error) { if (error.code !== "ENOENT") throw error; }
      }
    }
  }
  return bytes / gib;
}

export function reportBuildStorage(environment = process.env, {
  phase = "检查", requireFreeSpace = false, strictCache = false,
} = {}) {
  let healthy = true;
  try {
    const targetRoot = effectiveTauriTargetDir(environment);
    const minimum = threshold(environment, "BALANCEHUB_MIN_FREE_GIB", 20);
    const warning = threshold(environment, "BALANCEHUB_TARGET_WARN_GIB", 8);
    const disk = statfsSync(existingDirectory(targetRoot), { bigint: true });
    const available = Number(disk.bavail * disk.bsize) / gib;
    console.log(`构建空间检查（${phase}）：target=${targetRoot}；所在磁盘可用 ${available.toFixed(1)} GiB`);
    if (available < minimum) {
      console.warn(`磁盘可用空间不足 ${minimum} GiB；先清理未被占用的可再生产物，再运行重型构建。`);
      if (requireFreeSpace) healthy = false;
    }
    const roots = [
      ["当前 target", targetRoot],
      ["历史仓库 target", join(projectRoot, "src-tauri", "target")],
      ["默认 target", defaultTauriTargetDir(environment)],
      ["历史 doctor target", join(defaultTauriTargetDir(environment), "doctor")],
    ];
    const seen = new Set();
    for (const [label, path] of roots) {
      const key = process.platform === "linux" ? resolve(path) : resolve(path).toLowerCase();
      if (seen.has(key)) continue;
      seen.add(key);
      if (!existsSync(path)) continue;
      try {
        const size = targetSizeGiB(path);
        console.log(`构建缓存检查：${label} 当前约 ${size.toFixed(1)} GiB`);
        if (size > warning) {
          console.warn(`  超过 ${warning} GiB 复查阈值：${path}；确认占用后回收历史产物，避免反复全清重建。`);
          if (strictCache) healthy = false;
        }
        if (label.startsWith("历史") && size > 0) {
          console.warn(`  发现旧构建目录：${path}；后续构建复用当前 target，旧目录仅在确认未占用后清理。`);
        }
      } catch (error) {
        console.warn(`构建缓存体量暂不可读：${path}：${error.message}`);
        if (strictCache) healthy = false;
      }
    }
  } catch (error) {
    console.error(`构建空间检查失败：${error.message}`);
    healthy = false;
  }
  return healthy;
}
