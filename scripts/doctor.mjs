import { existsSync, readdirSync, readFileSync } from "node:fs";
import { join, relative } from "node:path";
import {
  localCargoEnv,
  projectRoot,
  reportBuildStorage,
} from "./build-cache.mjs";
import { runBuildCommand } from "./build-runner.mjs";

const sourceRoot = join(projectRoot, "src-tauri", "src");
const staticOnly = process.argv.includes("--static-only");
const strictCache = process.argv.includes("--strict-cache");

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

async function run(label, command, args, requiresBuildSpace = false) {
  console.log(`\n[doctor] ${label}`);
  const code = await runBuildCommand(commandName(command), args, {
    env: rustEnv, requiresBuildSpace,
  });
  if (code !== 0) {
    console.error(`[doctor] ${label}失败，退出码 ${code}`);
    return false;
  }
  return true;
}

const rustEnv = localCargoEnv();
const checks = [checkPlatformScriptNames(), reportBuildStorage(rustEnv, {
  phase: "自检前", requireFreeSpace: !staticOnly, strictCache,
})];
let healthy = checks.every(Boolean);
if (!staticOnly && healthy) {
  const steps = [
    ["Git 差异检查", "git", ["diff", "--check"]],
    ["前端构建", "npm", ["run", "build"]],
    ["前端测试", "npm", ["test"]],
    ["Rust 格式检查", "cargo", ["fmt", "--manifest-path", join(projectRoot, "src-tauri/Cargo.toml"), "--check"]],
    ["Rust Clippy", "cargo", ["clippy", "--manifest-path", join(projectRoot, "src-tauri/Cargo.toml"), "--locked", "--all-targets", "--all-features", "--", "-D", "warnings"], true],
    ["Rust 测试", "cargo", ["test", "--manifest-path", join(projectRoot, "src-tauri/Cargo.toml"), "--locked"], true],
  ];
  for (const step of steps) {
    if (!await run(...step)) {
      healthy = false;
      break;
    }
  }
  healthy = reportBuildStorage(rustEnv, { phase: "自检后", requireFreeSpace: true, strictCache }) && healthy;
}

if (healthy) {
  console.log("\nBalanceHub 自检通过");
} else {
  console.error("\nBalanceHub 自检发现问题");
  process.exitCode = 1;
}
