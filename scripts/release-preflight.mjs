import { execFileSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

import { readChangelog, releaseSection } from "../.github/scripts/changelog-section.mjs";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const argv = process.argv.slice(2);
const allowExistingTag = argv.includes("--allow-existing-tag");
const tagArgument = argv.find((argument) => !argument.startsWith("--"));

const packageVersion = readJson("package.json").version;
const tag = tagArgument ?? `v${packageVersion}`;
const tagVersion = tag.replace(/^v/i, "");
const failures = [];
const warnings = [];

if (tagVersion !== packageVersion) {
  failures.push(`tag ${tag} 与 package.json 版本 ${packageVersion} 不一致`);
}

checkVersionFiles();
checkChangelog();
checkTag();
checkWorktree();
printCommitReview();

for (const warning of warnings) {
  console.warn(`提示：${warning}`);
}

if (failures.length > 0) {
  for (const failure of failures) {
    console.error(`失败：${failure}`);
  }
  process.exitCode = 1;
} else {
  console.log(`发布预检通过：${tag}`);
}

function checkVersionFiles() {
  const files = [
    ["package.json", (value) => value.version],
    ["package-lock.json", (value) => value.version],
    ["package-lock.json", (value) => value.packages?.[""]?.version],
    ["src-tauri/Cargo.toml", (value) => cargoPackageVersion(value)],
    ["crates/core/Cargo.toml", (value) => cargoPackageVersion(value)],
    ["crates/agent/Cargo.toml", (value) => cargoPackageVersion(value)],
    ["crates/server/Cargo.toml", (value) => cargoPackageVersion(value)],
    ["src-tauri/tauri.conf.json", (value) => value.version],
  ];

  for (const [file, read] of files) {
    const value = read(readFile(file));
    if (value !== packageVersion) {
      failures.push(`${file} 版本为 ${value ?? "缺失"}，应为 ${packageVersion}`);
    }
  }

  const lock = readText("Cargo.lock");
  for (const name of ["balancehub", "balancehub-agent", "balancehub-core", "balancehub-server"]) {
    const match = lock.match(
      new RegExp(`\\[\\[package\\]\\]\\nname = "${name}"\\nversion = "([^"]+)"`),
    );
    if (match?.[1] !== packageVersion) {
      failures.push(`Cargo.lock 中 ${name} 版本为 ${match?.[1] ?? "缺失"}，应为 ${packageVersion}`);
    }
  }
}

function checkChangelog() {
  const section = releaseSection(readChangelog(path.join(root, "CHANGELOG.md")), packageVersion);
  if (!section) {
    failures.push(`CHANGELOG.md 缺少 ${packageVersion} 的独立章节`);
    return;
  }
  const body = section.replace(/^##.*$/m, "");
  if (!/^\s*[-*]\s+\S/m.test(body)) {
    failures.push(`CHANGELOG.md 的 ${packageVersion} 章节没有条目`);
  }
}

function checkTag() {
  const exists = git(["rev-parse", "--verify", "--quiet", `refs/tags/${tag}`], {
    allowFailure: true,
  });
  if (!exists) {
    return;
  }
  if (allowExistingTag || process.env.GITHUB_ACTIONS === "true") {
    console.log(`提示：tag ${tag} 已存在（发布流水线中属预期）`);
    return;
  }
  failures.push(`本地已存在 tag ${tag}；确认不是重复发布，或加 --allow-existing-tag`);
}

function checkWorktree() {
  if (git(["status", "--porcelain"])) {
    warnings.push("工作区有未提交改动，tag 不会包含这些内容");
  }
}

function printCommitReview() {
  const tagExists = Boolean(
    git(["rev-parse", "--verify", "--quiet", `refs/tags/${tag}`], { allowFailure: true }),
  );
  const end = tagExists ? tag : "HEAD";
  const start = git(["describe", "--tags", "--abbrev=0", "--match", "v*", `${end}^`], {
    allowFailure: true,
  });
  const range = start ? `${start}..${end}` : end;
  const commits = git(["log", "--format=%h %s", range])
    .split("\n")
    .filter(Boolean);

  console.log(`对照提交（${start ? range : "全部历史"}，${commits.length} 条）：`);
  for (const commit of commits) {
    console.log(`  ${commit}`);
  }
  if (commits.length === 0) {
    console.log("  （无）");
  }
  console.log("请逐条确认用户可见变更都已写入 CHANGELOG。");
}

function readFile(file) {
  return file.endsWith(".json") ? readJson(file) : readText(file);
}

function readJson(file) {
  return JSON.parse(readText(file));
}

function readText(file) {
  return fs.readFileSync(path.join(root, file), "utf8");
}

function cargoPackageVersion(text) {
  return text.match(/^\[package\][^[]*?^version\s*=\s*"([^"]+)"/m)?.[1];
}

function git(args, { allowFailure = false } = {}) {
  try {
    return execFileSync("git", args, {
      cwd: root,
      encoding: "utf8",
      stdio: ["ignore", "pipe", "pipe"],
    }).trim();
  } catch (error) {
    if (allowFailure) {
      return "";
    }
    throw error;
  }
}
