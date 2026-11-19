import assert from "node:assert/strict";
import test from "node:test";

import type {
  AgentCliKind,
  LivenessRecord,
  Provider,
} from "../src/stores/provider-types.ts";
import {
  LIVENESS_TIMELINE_SLOTS,
  latestAgentLivenessRecord,
  livenessTimestamp,
  livenessTimelineSlots,
  parseLivenessTimestamp,
  providerLivenessRecords,
  providerLivenessScope,
} from "../src/utils/provider-liveness.ts";

const minute = 60 * 1000;
const base = 1_760_000_000_000;

function record(checkedAt: number, ok: boolean, source = "automatic"): LivenessRecord {
  return {
    checkedAt: String(checkedAt),
    source,
    ok,
    latencyMs: 100,
    model: "",
    baseUrl: "",
    prompt: "",
    responsePreview: "",
    message: "",
    commandPreview: "",
  };
}

function scopedRecord(
  checkedAt: number,
  overrides: Partial<LivenessRecord>,
): LivenessRecord {
  return { ...record(checkedAt, true), ...overrides };
}

function provider(overrides: {
  cliKind?: AgentCliKind | null;
  model?: string;
  agentBaseUrls?: Partial<Record<AgentCliKind, string>>;
  records: LivenessRecord[];
}): Provider {
  return {
    identity: { baseUrl: "https://relay.example.com" },
    liveness: {
      cliKind: overrides.cliKind ?? null,
      model: overrides.model ?? "",
      agentBaseUrls: overrides.agentBaseUrls ?? {},
      records: overrides.records,
    },
  } as unknown as Provider;
}

test("24 次自动测活按时间顺序各占一格，最新一次在最右端", () => {
  const records = Array.from({ length: LIVENESS_TIMELINE_SLOTS }, (_, index) =>
    record(base + index * minute, index % 2 === 0),
  );

  const slots = livenessTimelineSlots([...records].reverse());

  assert.equal(slots.length, LIVENESS_TIMELINE_SLOTS);
  assert.ok(slots.every((slot) => slot !== null));
  assert.equal(slots[0]?.checkedAt, String(base));
  assert.equal(slots[LIVENESS_TIMELINE_SLOTS - 1]?.checkedAt, String(base + 23 * minute));
});

test("超过 24 次时只展示最近 24 次，旧记录让出格子", () => {
  const records = Array.from({ length: 30 }, (_, index) => record(base + index * minute, true));

  const slots = livenessTimelineSlots(records);

  assert.equal(slots[0]?.checkedAt, String(base + 6 * minute));
  assert.equal(slots[LIVENESS_TIMELINE_SLOTS - 1]?.checkedAt, String(base + 29 * minute));
});

test("不足 24 次时靠右排列，单次测活不占自动测活格子", () => {
  const records = [
    record(base, false),
    record(base + minute, true, "manual"),
    record(base + 2 * minute, true),
  ];

  const slots = livenessTimelineSlots(records);

  assert.equal(slots.filter((slot) => slot !== null).length, 2);
  assert.equal(slots[LIVENESS_TIMELINE_SLOTS - 2]?.checkedAt, String(base));
  assert.equal(slots[LIVENESS_TIMELINE_SLOTS - 1]?.checkedAt, String(base + 2 * minute));
});

test("测活线只统计当前 Agent、模型和地址的组合", () => {
  const target = provider({
    cliKind: "codex",
    model: "gpt-5",
    agentBaseUrls: { codex: "https://relay.example.com/v1" },
    records: [
      scopedRecord(base, {
        cliKind: "codex",
        model: "gpt-5",
        baseUrl: "https://relay.example.com/v1",
      }),
      scopedRecord(base + minute, {
        cliKind: "claudeCode",
        model: "gpt-5",
        baseUrl: "https://relay.example.com/v1",
      }),
      scopedRecord(base + 2 * minute, {
        cliKind: "codex",
        model: "gpt-5-mini",
        baseUrl: "https://relay.example.com/v1",
      }),
      scopedRecord(base + 3 * minute, {
        cliKind: "codex",
        model: "gpt-5",
        baseUrl: "https://backup.example.com/v1",
      }),
    ],
  });

  const scope = providerLivenessScope(target);
  assert.equal(scope.cliKind, "codex");
  assert.equal(scope.model, "gpt-5");
  assert.equal(scope.baseUrl, "https://relay.example.com/v1");

  const records = providerLivenessRecords(target);
  assert.equal(records.length, 1);
  assert.equal(records[0]?.checkedAt, String(base));
});

test("换目标后只统计新目标的记录，旧模型结果不会冒充当前测活", () => {
  const target = provider({
    cliKind: "codex",
    model: "gpt-5",
    records: [
      scopedRecord(base, { cliKind: "codex", model: "claude-sonnet-4" }),
      scopedRecord(base + minute, { cliKind: "claudeCode", model: "claude-sonnet-4" }),
    ],
  });

  assert.deepEqual(providerLivenessRecords(target), []);
  // 原始记录仍在，测活明细可以继续展示其他目标的历史。
  assert.equal(target.liveness.records?.length, 2);
});

test("跟随全局且未声明 Agent 和模型时保留全部记录", () => {
  const target = provider({
    records: [
      scopedRecord(base, { cliKind: "codex", model: "gpt-5" }),
      scopedRecord(base + minute, { cliKind: "claudeCode", model: "claude-sonnet-4" }),
    ],
  });

  assert.equal(providerLivenessScope(target).cliKind, null);
  assert.equal(providerLivenessRecords(target).length, 2);
});

test("详情拿得到全局设置时，按后端实际目标过滤记录", () => {
  const target = provider({
    agentBaseUrls: { codex: "https://relay.example.com/v1" },
    records: [
      scopedRecord(base, {
        cliKind: "codex",
        model: "gpt-5",
        baseUrl: "https://relay.example.com/v1",
      }),
      scopedRecord(base + minute, {
        cliKind: "codex",
        model: "claude-sonnet-4",
        baseUrl: "https://relay.example.com/v1",
      }),
      scopedRecord(base + 2 * minute, {
        cliKind: "claudeCode",
        model: "gpt-5",
        baseUrl: "https://relay.example.com/v1",
      }),
    ],
  });
  const settings = { livenessCliKind: "codex", livenessModel: "gpt-5" } as const;

  // 后端 effective_cli_kind / effective_model 在卡片声明为空时回落到全局，
  // 详情摘要必须用同一个目标，不能把其他模型的旧记录算进来。
  const scope = providerLivenessScope(target, settings);
  assert.equal(scope.cliKind, "codex");
  assert.equal(scope.model, "gpt-5");
  assert.equal(scope.baseUrl, "https://relay.example.com/v1");
  assert.deepEqual(
    providerLivenessRecords(target, settings).map((item) => item.checkedAt),
    [String(base)],
  );
});

test("卡片自己的测活目标优先于全局设置", () => {
  const target = provider({
    cliKind: "claudeCode",
    model: "claude-sonnet-4",
    records: [
      scopedRecord(base, { cliKind: "claudeCode", model: "claude-sonnet-4" }),
      scopedRecord(base + minute, { cliKind: "codex", model: "gpt-5" }),
    ],
  });
  const settings = { livenessCliKind: "codex", livenessModel: "gpt-5" } as const;

  assert.deepEqual(
    providerLivenessRecords(target, settings).map((item) => item.checkedAt),
    [String(base)],
  );
});

test("没有 Agent 归属的旧记录不混入已指定 Agent 的测活线", () => {
  const target = provider({
    cliKind: "codex",
    model: "gpt-5",
    records: [
      // 旧记录只有模型，没有 cliKind；无法证明它属于当前 Agent。
      scopedRecord(base, { model: "gpt-5" }),
      scopedRecord(base + minute, { cliKind: "codex", model: "gpt-5" }),
    ],
  });

  assert.deepEqual(
    providerLivenessRecords(target).map((item) => item.checkedAt),
    [String(base + minute)],
  );
});

test("模型映射后，记录里的上游名仍能对回卡片上的模型 ID", () => {
  const target = provider({
    cliKind: "codex",
    model: "gpt-5",
    records: [
      scopedRecord(base, {
        cliKind: "codex",
        model: "gpt-5-2026",
        modelId: "gpt-5",
      }),
      // 旧记录没有 modelId，只能靠 wireName 反查。
      scopedRecord(base + minute, { cliKind: "codex", model: "gpt-5-2026" }),
      scopedRecord(base + 2 * minute, { cliKind: "codex", model: "other-model" }),
    ],
  });
  target.modelConfig = {
    overrides: { "gpt-5": { wireName: "gpt-5-2026" } },
  } as unknown as Provider["modelConfig"];

  assert.deepEqual(
    providerLivenessRecords(target).map((item) => item.checkedAt),
    [String(base), String(base + minute)],
  );
});

test("Agent 地址会补 /v1 时，原始站点地址仍能匹配记录", () => {
  const target = provider({
    cliKind: "codex",
    model: "gpt-5",
    agentBaseUrls: { codex: "https://relay.example.com" },
    records: [
      scopedRecord(base, {
        cliKind: "codex",
        model: "gpt-5",
        baseUrl: "https://relay.example.com/v1",
      }),
      scopedRecord(base + minute, {
        cliKind: "codex",
        model: "gpt-5",
        baseUrl: "https://other.example.com/v1",
      }),
    ],
  });

  assert.deepEqual(
    providerLivenessRecords(target).map((item) => item.checkedAt),
    [String(base)],
  );
});

test("记录里带有原始站点地址时优先用它匹配", () => {
  const target = provider({
    cliKind: "codex",
    model: "gpt-5",
    agentBaseUrls: { codex: "https://relay.example.com/openai" },
    records: [
      scopedRecord(base, {
        cliKind: "codex",
        model: "gpt-5",
        baseUrl: "https://relay.example.com/openai/v1",
        providerBaseUrl: "https://relay.example.com/openai",
      }),
      scopedRecord(base + minute, {
        cliKind: "codex",
        model: "gpt-5",
        baseUrl: "https://relay.example.com/openai/v1",
        providerBaseUrl: "https://relay.example.com/other",
      }),
    ],
  });

  assert.deepEqual(
    providerLivenessRecords(target).map((item) => item.checkedAt),
    [String(base)],
  );
});

test("新的预检失败会盖过更早的成功记录", () => {
  const target = provider({
    records: [
      scopedRecord(base, {
        cliKind: "codex",
        model: "gpt-5",
        baseUrl: "https://relay.example.com/v1",
        ok: true,
      }),
      scopedRecord(base + minute, {
        cliKind: "",
        model: "",
        baseUrl: "",
        ok: false,
        message: "缺少 API Key，测活需要 API Key",
      }),
    ],
  });

  const latest = latestAgentLivenessRecord(
    target,
    "codex",
    "gpt-5",
    "https://relay.example.com/v1",
  );
  assert.equal(latest?.ok, false, "预检失败没有模型信息，但不能被更早的成功盖住");
  assert.equal(latest?.message, "缺少 API Key，测活需要 API Key");
});

test("毫秒数字串和 RFC3339 混排时仍按真实时间排序", () => {
  const iso = "2026-11-18T08:30:00Z";
  const isoMillis = Date.parse(iso);
  assert.equal(parseLivenessTimestamp(iso), isoMillis);

  const records = [
    { ...record(isoMillis + minute, true), checkedAt: iso },
    record(isoMillis + 2 * minute, false),
  ];
  const slots = livenessTimelineSlots(records).filter(
    (item): item is LivenessRecord => item !== null,
  );
  assert.deepEqual(
    slots.map((item) => item.ok),
    [true, false],
    "ISO 时间不能被当成 NaN 丢掉或排到最后",
  );
  assert.ok(livenessTimestamp(records[0]!) < livenessTimestamp(records[1]!));
  assert.equal(parseLivenessTimestamp("not-a-time"), 0);
});

test("绑定健康只取当前 Agent 的记录，并优先匹配模型和地址", () => {
  const target = provider({
    records: [
      scopedRecord(base, { cliKind: "codex", model: "gpt-5", baseUrl: "https://relay.example.com/v1", ok: true }),
      scopedRecord(base + minute, { cliKind: "codex", model: "gpt-5", baseUrl: "https://relay.example.com/v1/", ok: false }),
      scopedRecord(base + 2 * minute, { cliKind: "claudeCode", model: "gpt-5", baseUrl: "https://relay.example.com/v1", ok: false }),
    ],
  });

  const exact = latestAgentLivenessRecord(
    target,
    "codex",
    "gpt-5",
    "https://relay.example.com/v1",
  );
  assert.equal(exact?.checkedAt, String(base + minute), "末尾斜杠不应破坏地址匹配");
  assert.equal(exact?.ok, false);

  const otherModel = latestAgentLivenessRecord(target, "codex", "claude-sonnet-4");
  assert.equal(otherModel?.checkedAt, String(base + minute), "没有目标模型时退回同一 Agent");
  assert.equal(latestAgentLivenessRecord(target, "geminiCli", "gpt-5"), null);
});
