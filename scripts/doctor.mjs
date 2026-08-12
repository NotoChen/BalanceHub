import { execFileSync, spawnSync } from "node:child_process";
import { existsSync, readdirSync, readFileSync, statSync } from "node:fs";
import { join, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import {
  defaultTauriTargetDir,
  doctorTauriTargetDir,
  effectiveTauriTargetDir,
} from "./build-cache.mjs";

const projectRoot = resolve(fileURLToPath(new URL("..", import.meta.url)));
const sourceRoot = join(projectRoot, "src-tauri", "src");
const repositoryTargetRoot = join(projectRoot, "src-tauri", "target");
const staticOnly = process.argv.includes("--static-only");
const strictCache = process.argv.includes("--strict-cache");
const targetWarnGiB = Number(process.env.BALANCEHUB_TARGET_WARN_GIB || "8");

const unixShellReserved = new Set([
  "status",
  "pipestatus",
  "argv",
  "commands",
  "funcstack",
  "history",
  "options",
  "signals",
  "words",
  "PWD",
  "OLDPWD",
  "PPID",
]);
const powerShellReserved = new Set([
  "PID",
  "HOME",
  "PWD",
  "LASTEXITCODE",
  "ERROR",
  "PSHOME",
  "PSCOMMANDPATH",
  "PSVERSIONTABLE",
  "true",
  "false",
  "null",
]);
const cmdReserved = new Set([
  "ERRORLEVEL",
  "CD",
  "DATE",
  "TIME",
  "RANDOM",
  "CMDEXTVERSION",
  "CMDCMDLINE",
]);

function sourceFiles(directory) {
  if (!existsSync(directory)) return [];
  const files = [];
  for (const entry of readdirSync(directory, { withFileTypes: true })) {
    const path = join(directory, entry.name);
    if (entry.isDirectory()) files.push(...sourceFiles(path));
    else if (entry.isFile() && path.endsWith(".rs")) files.push(path);
  }
  return files;
}

function sourceLine(text, offset) {
  return text.slice(0, offset).split("\n").length;
}

function checkPlatformScriptNames() {
  const findings = [];
  const unixAssignment = /(?:^|\n)[ \t]*([A-Za-z_][A-Za-z0-9_]*)[ \t]*=[ \t]*\$\?/g;
  const powerShellAssignment = /\$([A-Za-z_][A-Za-z0-9_]*)[ \t]*=/g;
  const cmdAssignment = /\bset[ \t]+\"?([A-Za-z_][A-Za-z0-9_]*)[ \t]*=/gi;

  for (const path of sourceFiles(sourceRoot)) {
    const text = readFileSync(path, "utf8");
    for (const match of text.matchAll(unixAssignment)) {
      const name = match[1];
      if (unixShellReserved.has(name)) {
        findings.push(`${relative(projectRoot, path)}:${sourceLine(text, match.index)} Unix shell 保留变量 ${name}`);
      }
    }
    for (const match of text.matchAll(powerShellAssignment)) {
      const name = match[1];
      if (powerShellReserved.has(name.toUpperCase()) || powerShellReserved.has(name)) {
        findings.push(`${relative(projectRoot, path)}:${sourceLine(text, match.index)} PowerShell 自动变量 $${name}`);
      }
    }
    for (const match of text.matchAll(cmdAssignment)) {
      const name = match[1].toUpperCase();
      if (cmdReserved.has(name)) {
        findings.push(`${relative(projectRoot, path)}:${sourceLine(text, match.index)} cmd 保留变量 ${name}`);
      }
    }
  }

  if (findings.length > 0) {
    console.error("平台脚本变量检查失败：");
    for (const finding of findings) console.error(`  - ${finding}`);
    return false;
  }
  console.log("平台脚本变量检查通过");
  return true;
}

function commandName(command) {
  return process.platform === "win32" && command === "npm" ? "npm.cmd" : command;
}

function run(label, command, args, options = {}) {
  console.log(`\n[doctor] ${label}`);
  const result = spawnSync(commandName(command), args, {
    cwd: projectRoot,
    stdio: "inherit",
    env: process.env,
    shell: process.platform === "win32",
    ...options,
  });
  if (result.error) {
    console.error(`[doctor] 无法执行 ${command}: ${result.error.message}`);
    return false;
  }
  if (result.status !== 0) {
    console.error(`[doctor] ${label}失败，退出码 ${result.status ?? "未知"}`);
    return false;
  }
  return true;
}

function targetSizeGiB(targetRoot) {
  if (!existsSync(targetRoot)) return 0;
  if (process.platform !== "win32") {
    try {
      const output = execFileSync("du", ["-sk", targetRoot], { encoding: "utf8" });
      const kib = Number.parseInt(output.trim().split(/\s+/)[0], 10);
      return Number.isFinite(kib) ? kib / (1024 * 1024) : 0;
    } catch {
      return 0;
    }
  }

  let bytes = 0;
  const stack = [targetRoot];
  while (stack.length > 0) {
    const directory = stack.pop();
    for (const entry of readdirSync(directory, { withFileTypes: true })) {
      const path = join(directory, entry.name);
      if (entry.isDirectory()) stack.push(path);
      else if (entry.isFile()) {
        try {
          bytes += statSync(path).size;
        } catch {
          // A concurrent compiler may remove a file while the report runs.
        }
      }
    }
  }
  return bytes / (1024 ** 3);
}

function reportBuildCache() {
  const roots = [
    ["仓库 target", repositoryTargetRoot],
    ["当前开发 target", effectiveTauriTargetDir()],
    ["默认开发 target", defaultTauriTargetDir()],
  ];
  let healthy = true;
  const seen = new Set();
  for (const [label, targetRoot] of roots) {
    if (seen.has(targetRoot)) continue;
    seen.add(targetRoot);
    const size = targetSizeGiB(targetRoot);
    if (size <= 0) {
      console.log(`构建缓存检查：${label} 不存在`);
      continue;
    }
    const sizeLabel = `${size.toFixed(1)} GiB`;
    if (size > targetWarnGiB) {
      healthy = false;
      console.warn(`构建缓存检查：${label} 当前约 ${sizeLabel}，超过 ${targetWarnGiB} GiB 软阈值`);
      console.warn(`  清理前确认没有运行中的开发构建，再执行：cargo clean --manifest-path ${join(projectRoot, "src-tauri/Cargo.toml")} --target-dir ${targetRoot}`);
    } else {
      console.log(`构建缓存检查：${label} 当前约 ${sizeLabel}`);
    }
  }
  return healthy || !strictCache;
}

const checks = [checkPlatformScriptNames(), reportBuildCache()];
const rustEnv = {
  ...process.env,
  CARGO_TARGET_DIR: process.env.CARGO_TARGET_DIR?.trim() || doctorTauriTargetDir(),
  CARGO_INCREMENTAL: "0",
};
if (!staticOnly) {
  checks.push(
    run("Git 差异检查", "git", ["diff", "--check"]),
    run("前端构建", "npm", ["run", "build"]),
    run("前端测试", "npm", ["test"]),
    run("Rust 格式检查", "cargo", ["fmt", "--manifest-path", join(projectRoot, "src-tauri/Cargo.toml"), "--check"], { env: rustEnv }),
    run("Rust Clippy", "cargo", ["clippy", "--manifest-path", join(projectRoot, "src-tauri/Cargo.toml"), "--locked", "--all-targets", "--all-features", "--", "-D", "warnings"], { env: rustEnv }),
    run("Rust 测试", "cargo", ["test", "--manifest-path", join(projectRoot, "src-tauri/Cargo.toml"), "--locked"], { env: rustEnv }),
  );
}

if (checks.every(Boolean)) {
  console.log("\nBalanceHub 自检通过");
} else {
  console.error("\nBalanceHub 自检发现问题");
  process.exitCode = 1;
}
