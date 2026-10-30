import assert from "node:assert/strict";
import test from "node:test";
import { providerModelChips } from "../src/utils/provider-models.ts";

test("model chips spend the first pass on distinct identifiers, newest version first", () => {
  const catalog = [
    "gpt-4o",
    "gpt-4o-mini",
    "gpt-5.2",
    "gpt-5.2-luna",
    "claude-sonnet-4-5",
    "claude-opus-4-6",
    "deepseek-v3.2",
    "deepseek-r1",
  ];

  // 第一轮每个标识只出一条，且取版本最高的那条：卡片只有三行，先保证
  // 用户看到的是「有哪几个型号」而不是同一家族的一串版本号。
  assert.deepEqual(providerModelChips(catalog, 4).names, [
    "gpt-5.2",
    "claude-opus-4-6",
    "claude-sonnet-4-5",
    "deepseek-v3.2",
  ]);

  // 第二轮才回到同标识的次高版本，并且基础型号先于带修饰词的变体。
  assert.deepEqual(providerModelChips(catalog, 7).names, [
    "gpt-5.2",
    "claude-opus-4-6",
    "claude-sonnet-4-5",
    "deepseek-v3.2",
    "gpt-5.2-luna",
    "deepseek-r1",
    "gpt-4o",
  ]);
  assert.equal(providerModelChips(catalog, 4).total, catalog.length);
});

test("a higher numeric version outranks a lower one within the same identifier", () => {
  assert.equal(
    providerModelChips(["deepseek-v2.5", "deepseek-v3.2"], 1).names[0],
    "deepseek-v3.2",
  );
  // 缺少的版本位按 -1 计，`5` 仍然高于 `4.5`。
  assert.equal(
    providerModelChips(["claude-sonnet-4.5", "claude-sonnet-5"], 1).names[0],
    "claude-sonnet-5",
  );
});

test("the vendor prefix keeps two vendors' same-named models apart", () => {
  const chips = providerModelChips(
    ["openai/gpt-5", "anthropic/gpt-5", "openai/gpt-4"],
    3,
  );
  assert.deepEqual(chips.names.slice(0, 2).sort(), [
    "anthropic/gpt-5",
    "openai/gpt-5",
  ]);
});

test("duplicates collapse case-insensitively and empty input stays empty", () => {
  assert.deepEqual(providerModelChips(["GPT-5", " gpt-5 ", "gpt-5"]).names, [
    "GPT-5",
  ]);
  assert.deepEqual(providerModelChips(undefined), { names: [], total: 0 });
  assert.deepEqual(providerModelChips([], 0), { names: [], total: 0 });
});

test("unversioned variants of one identifier count as one identifier", () => {
  // `jev-latest` / `jev-preview` 没有版本号可提，但仍是同一个标识；三行要留给
  // 更多型号，不能两行都写 jev。
  const chips = providerModelChips(
    ["jev-latest", "jev-preview", "glm-4.5", "kimi-k3"],
    3,
  );
  assert.equal(chips.names.filter((name) => name.startsWith("jev-")).length, 1);
  assert.equal(chips.names.length, 3);
  assert.equal(chips.total, 4);
});
