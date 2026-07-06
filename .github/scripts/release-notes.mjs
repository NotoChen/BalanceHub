import fs from "node:fs";

const tagName = process.argv[2] ?? "";
const version = tagName.replace(/^v/i, "").trim();

if (!version) {
  throw new Error("缺少版本号参数，例如 v0.1.0");
}

const changelog = fs.readFileSync("CHANGELOG.md", "utf8");
const headingPattern = new RegExp(`^##\\s+${escapeRegExp(version)}(?:\\s+-.*)?$`, "m");
const match = changelog.match(headingPattern);

if (!match || match.index === undefined) {
  throw new Error(`CHANGELOG.md 中缺少 ${version} 的独立更新说明`);
}

const start = match.index;
const nextHeading = changelog.slice(start + match[0].length).search(/^##\s+/m);
const section =
  nextHeading === -1
    ? changelog.slice(start).trim()
    : changelog.slice(start, start + match[0].length + nextHeading).trim();

const installNotes = [
  "## 安装包说明",
  "",
  "- macOS：下载 `.dmg`。",
  "- Windows：下载 `setup.exe`。",
  "- Linux：下载 `.AppImage`、`.deb` 或 `.rpm`。",
  "- `.sig` 文件和 `latest.json` 用于应用内自动更新，不是手动安装入口。",
].join("\n");

writeOutput("release_body", `${section}\n\n${installNotes}`);

function escapeRegExp(value) {
  return value.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

function writeOutput(name, value) {
  const outputPath = process.env.GITHUB_OUTPUT;
  if (!outputPath) {
    console.log(value);
    return;
  }

  const delimiter = `EOF_${Date.now()}_${Math.random().toString(16).slice(2)}`;
  fs.appendFileSync(outputPath, `${name}<<${delimiter}\n${value}\n${delimiter}\n`);
}
