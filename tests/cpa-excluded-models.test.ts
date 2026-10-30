import assert from "node:assert/strict";
import test from "node:test";
import {
  getModelExclusionState,
  isModelExcluded,
  matchesExcludedRule,
  normalizeExcludedRules,
  parseExcludedRulesText,
  replaceCustomExcludedRules,
  sameExcludedRules,
  splitExcludedRules,
  summarizeExclusion,
  toggleExcludedRule,
} from "../src/utils/cpa-excluded-models.ts";

test("excluded rules are trimmed, deduped by lowercased key and keep the first spelling", () => {
  assert.deepEqual(
    normalizeExcludedRules([" GPT-5 ", "gpt-5", "", "Opus", "opus"]),
    ["GPT-5", "Opus"],
  );
});

test("only * is a wildcard; a dot in a model ID stays literal", () => {
  assert.equal(matchesExcludedRule("gpt-4.1", "gpt-4x1"), false);
  assert.equal(matchesExcludedRule("gpt-4.1", "GPT-4.1"), true);
  assert.equal(matchesExcludedRule("gpt-5-*", "gpt-5-mini"), true);
  assert.equal(matchesExcludedRule("*-mini", "gpt-5-mini"), true);
  assert.equal(matchesExcludedRule("*", "anything"), true);
  assert.equal(matchesExcludedRule("gpt-5-*", "gpt-6-mini"), false);
});

test("a rule is either a catalog tick or custom text, never both", () => {
  assert.deepEqual(
    splitExcludedRules(["Opus", "claude-3-*", "retired-model"], ["opus", "haiku"]),
    {
      exactRules: ["opus"],
      wildcardRules: ["claude-3-*"],
      unknownRules: ["retired-model"],
      customRules: ["claude-3-*", "retired-model"],
    },
  );
});

test("replacing the custom text keeps the catalog ticks", () => {
  assert.deepEqual(
    replaceCustomExcludedRules(["opus", "claude-3-*"], ["opus", "haiku"], "haiku\ngpt-5-*"),
    ["opus", "haiku", "gpt-5-*"],
  );
});

test("exclusion state separates an exact tick from a wildcard hit", () => {
  assert.deepEqual(getModelExclusionState([], "opus"), { state: "included" });
  assert.deepEqual(getModelExclusionState(["Opus"], "opus"), {
    state: "excluded",
    by: "exact",
  });
  assert.deepEqual(getModelExclusionState(["claude-3-*"], "claude-3-opus"), {
    state: "excluded",
    by: "wildcard",
    rule: "claude-3-*",
  });
  assert.deepEqual(
    getModelExclusionState(["claude-3-opus", "claude-3-*"], "claude-3-opus"),
    { state: "excluded", by: "both", rule: "claude-3-*" },
  );
});

test("summaries count models, not rules", () => {
  const ids = ["gpt-5", "gpt-5-mini", "gpt-5-nano"];
  assert.deepEqual(summarizeExclusion(["gpt-5-*"], ids), {
    total: 3,
    excluded: 2,
  });
  assert.equal(isModelExcluded(["gpt-5"], ids[0]), true);
});

test("toggling an exact rule keeps other rules and order", () => {
  assert.deepEqual(toggleExcludedRule(["opus", "haiku"], "HAIKU", false), ["opus"]);
  assert.deepEqual(toggleExcludedRule(["opus"], "haiku", true), ["opus", "haiku"]);
  assert.deepEqual(toggleExcludedRule(["opus"], "opus", true), ["opus"]);
});

test("two rule lists compare equal regardless of order and case", () => {
  assert.equal(sameExcludedRules(["Opus", "haiku"], ["haiku", "opus"]), true);
  assert.equal(sameExcludedRules(["opus"], ["opus", "haiku"]), false);
  assert.equal(parseExcludedRulesText("opus\n\n Opus \nhaiku").length, 2);
});
