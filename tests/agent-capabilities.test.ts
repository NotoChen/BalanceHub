import assert from "node:assert/strict";
import fs from "node:fs";
import test from "node:test";

import { agentCliVisuals } from "../src/agent-cli/visuals.ts";

type Descriptor = { kind: string };

const snapshot: Descriptor[] = JSON.parse(
  fs.readFileSync(
    new URL(
      "../crates/agent/src/services/agent_cli/capabilities.snapshot.json",
      import.meta.url,
    ),
    "utf8",
  ),
);

test("每个注册的 Agent 都有前端视觉条目", () => {
  for (const descriptor of snapshot) {
    assert.ok(
      Object.prototype.hasOwnProperty.call(agentCliVisuals, descriptor.kind),
      `缺少 ${descriptor.kind} 的 visuals 条目`,
    );
  }
  assert.equal(
    Object.keys(agentCliVisuals).length,
    snapshot.length,
    "visuals 中存在未注册的 Agent 条目",
  );
});
