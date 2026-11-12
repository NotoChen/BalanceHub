import assert from "node:assert/strict";
import test from "node:test";
import { requestLogRateHint } from "../src/utils/request-log-display.ts";

test("request log rate hints name the statistics window each protocol actually uses", () => {
  // Sub2API 的速率来自全站近 5 分钟看板，不跟筛选条件。
  assert.match(requestLogRateHint("sub2Api"), /近 5 分钟/);
  assert.match(requestLogRateHint("sub2Api"), /不受时间范围和关键词筛选影响/);
  // NewAPI 的统计接口与列表使用同一段窗口和模型筛选。
  assert.match(requestLogRateHint("newApi"), /所选时间范围/);
  assert.doesNotMatch(requestLogRateHint("newApi"), /近 5 分钟/);
});
