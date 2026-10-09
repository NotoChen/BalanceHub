import assert from "node:assert/strict";
import test from "node:test";
import {
  apiKeyRestrictionLines,
  apiKeySettingsPatch,
} from "../src/utils/provider-api-key-settings.ts";
import { keyEditorContext } from "./helpers/api-key-fixture.ts";

test("Key group edits omit untouched quota, expiration and restrictions", () => {
  const context = keyEditorContext();
  const draft = structuredClone(context.settings);
  draft.group = "group-b";
  assert.deepEqual(apiKeySettingsPatch(context, draft, false), {
    group: "group-b",
  });
  draft.allowIps = [];
  assert.deepEqual(apiKeySettingsPatch(context, draft, false), {
    group: "group-b",
    allowIps: [],
  });
  assert.equal(context.settings.allowIps.length, 1);
});

test("Key creation projects only the settings advertised by the backend", () => {
  const context = keyEditorContext();
  const patch = apiKeySettingsPatch(
    context,
    context.settings,
    true,
    "must-not-be-submitted",
  );
  assert.equal(patch.modelLimitsEnabled, true);
  for (const field of [
    "denyIps",
    "spendingLimits",
    "customKey",
    "autoGroups",
    "enabled",
  ])
    assert.equal(Object.hasOwn(patch, field), false);
  const sub2 = {
    ...context,
    supportsModelLimits: false,
    supportsCrossGroupRetry: false,
    supportsIpBlacklist: true,
    supportsSpendingLimits: true,
    supportsCustomKey: true,
    automaticGroup: null,
  };
  const sub2Patch = apiKeySettingsPatch(
    sub2,
    sub2.settings,
    true,
    "custom-fixture-key",
  );
  assert.deepEqual(sub2Patch.denyIps, []);
  assert.equal(sub2Patch.customKey, "custom-fixture-key");
  assert.equal(Object.hasOwn(sub2Patch, "modelLimits"), false);
});

test("Key IP restrictions accept multiline input and preserve CIDR ranges", () => {
  assert.deepEqual(
    apiKeyRestrictionLines("10.0.0.0/8\n\n ::1,10.0.0.0/8\r\n2001:db8::/32\n"),
    ["10.0.0.0/8", "::1", "2001:db8::/32"],
  );
});
