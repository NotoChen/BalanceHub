import assert from "node:assert/strict";
import test from "node:test";

import { releaseSection } from "../.github/scripts/changelog-section.mjs";

const changelog = [
  "# 更新记录",
  "",
  "## 0.6.12",
  "",
  "### 新增",
  "",
  "- 新功能",
  "",
  "## 0.6.11",
  "",
  "- 旧功能",
].join("\n");

test("提取指定版本的完整章节", () => {
  const section = releaseSection(changelog, "0.6.12");

  assert.ok(section);
  assert.match(section, /^## 0\.6\.12/m);
  assert.match(section, /新功能/);
  assert.doesNotMatch(section, /旧功能/);
});

test("缺少版本时返回 null", () => {
  assert.equal(releaseSection(changelog, "0.6.10"), null);
});
