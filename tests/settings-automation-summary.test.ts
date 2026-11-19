import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";

test("automation settings disclose what each automatic action will actually do", () => {
  const source = readFileSync(
    new URL(
      "../src/components/settings/SettingsAutomationSection.vue",
      import.meta.url,
    ),
    "utf8",
  );

  // 三个自动行为各有开关，摘要必须同时说明「开」和「关」时的真实行为。
  assert.match(source, /自动行为摘要/);
  assert.match(source, /key: "refresh"/);
  assert.match(source, /key: "checkIn"/);
  assert.match(source, /key: "liveness"/);
  assert.match(source, /formatDuration\(settings\.refreshInterval\)/);
  assert.match(source, /settings\.checkInTime \|\| "00:00"/);
  assert.match(source, /agentCliLabel\(store\.cliEnvironmentProbe, settings\.livenessCliKind\)/);
  // 浏览器版没有 Agent CLI 链路，摘要也不该虚构一条测活计划。
  assert.match(source, /if \(livenessAvailable\.value\)/);
  assert.match(source, /item\.on \? "开" : "关"/);
});
