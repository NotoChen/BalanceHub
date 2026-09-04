import fs from "node:fs";
import path from "node:path";

const repositoryRoot = process.cwd();
const sourcePath = path.join(repositoryRoot, "CHANGELOG.md");
const destinationPath = path.join(repositoryRoot, "docs", "changelog.md");

const changelog = fs.readFileSync(sourcePath, "utf8").trim();

if (!changelog.startsWith("# 更新记录")) {
  throw new Error("CHANGELOG.md 必须以一级标题“更新记录”开头");
}

const frontMatter = [
  "---",
  "layout: default",
  "title: 更新记录",
  "description: 按版本查看 BalanceHub 的新增能力、行为变化和问题修复。",
  "page_class: document-page",
  "permalink: /changelog.html",
  "---",
  "",
].join("\n");

fs.writeFileSync(destinationPath, `${frontMatter}${changelog}\n`, "utf8");
console.log(`已生成 ${path.relative(repositoryRoot, destinationPath)}`);
