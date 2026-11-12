import fs from "node:fs";

import { readChangelog, releaseSection } from "./changelog-section.mjs";

const tagName = process.argv[2] ?? "";
const version = tagName.replace(/^v/i, "").trim();

if (!version) {
  throw new Error("缺少版本号参数，例如 v0.1.0");
}

const section = releaseSection(readChangelog(), version);

if (!section) {
  throw new Error(`CHANGELOG.md 中缺少 ${version} 的独立更新说明`);
}

const installNotes = [
  "## 安装包说明",
  "",
  "- macOS：下载 `.dmg`。",
  "- Windows：下载 `setup.exe`。",
  "- Linux：下载 `.AppImage`、`.deb` 或 `.rpm`。",
  "- `.sig` 文件和 `latest.json` 用于应用内自动更新，不是手动安装入口。",
].join("\n");

writeOutput("release_body", `${section}\n\n${installNotes}`);

function writeOutput(name, value) {
  const outputPath = process.env.GITHUB_OUTPUT;
  if (!outputPath) {
    console.log(value);
    return;
  }

  const delimiter = `EOF_${Date.now()}_${Math.random().toString(16).slice(2)}`;
  fs.appendFileSync(outputPath, `${name}<<${delimiter}\n${value}\n${delimiter}\n`);
}
