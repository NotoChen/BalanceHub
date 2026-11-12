import fs from "node:fs";

export function readChangelog(file = "CHANGELOG.md") {
  return fs.readFileSync(file, "utf8");
}

/**
 * 取出指定版本的完整章节（含标题），供发布说明生成与发布预检共用。
 * 版本不存在时返回 null。
 */
export function releaseSection(changelog, version) {
  const headingPattern = new RegExp(`^##\\s+${escapeRegExp(version)}(?:\\s+-.*)?$`, "m");
  const match = changelog.match(headingPattern);

  if (!match || match.index === undefined) {
    return null;
  }

  const start = match.index;
  const nextHeading = changelog.slice(start + match[0].length).search(/^##\s+/m);
  return nextHeading === -1
    ? changelog.slice(start).trim()
    : changelog.slice(start, start + match[0].length + nextHeading).trim();
}

function escapeRegExp(value) {
  return value.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}
