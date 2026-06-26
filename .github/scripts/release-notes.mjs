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

const body = `${section}

## 安装包说明

| 文件类型 | 适用系统 | 说明 |
| --- | --- | --- |
| \`.dmg\` | macOS Apple Silicon / Intel | macOS 图形安装包。按文件名中的架构选择对应版本。 |
| \`setup.exe\` | Windows x64 / ARM64 | Windows NSIS 安装包。按文件名中的架构选择对应版本。 |
| \`.AppImage\` | Linux x64 / ARM64 | 通用 Linux 便携包，适合多数非 deb / rpm 发行版。 |
| \`.deb\` | Debian / Ubuntu 系发行版 | Debian 系安装包。 |
| \`.rpm\` | Fedora / RHEL / openSUSE 系发行版 | RPM 系安装包。 |
| \`.sig\` | 应用内自动更新 | Tauri updater 签名文件，不是手动安装入口。 |
| \`latest.json\` | 应用内自动更新 | 自动更新元数据，应用会根据当前系统选择匹配安装包。 |

## 自动更新

应用内检查更新会读取本 Release 的 \`latest.json\`，下载当前平台匹配的安装包，并用对应 \`.sig\` 文件完成签名校验后安装。`;

writeOutput("release_body", body);

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
