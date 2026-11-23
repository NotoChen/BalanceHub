import assert from "node:assert/strict";
import test from "node:test";
import { adoptSyncedDraftFields } from "../src/utils/provider-draft-baseline.ts";
import type { ProviderInput } from "../src/stores/provider-types.ts";

function draft(overrides: {
  baseUrl?: string;
  apiKey?: string;
  accessToken?: string;
} = {}) {
  return {
    identity: {
      id: "fixture",
      name: "测试站点",
      protocol: "newapi",
      baseUrl: overrides.baseUrl ?? "https://old.example.com",
    },
    auth: {
      mode: "accessToken",
      apiKey: overrides.apiKey ?? "sk-old",
      apiKeyTokenId: "token-1",
      accessToken: overrides.accessToken ?? "access-old",
      refreshToken: "refresh-old",
    },
    automation: { refreshInterval: 0 },
  } as unknown as ProviderInput;
}

function dirty(baseline: ProviderInput, current: ProviderInput) {
  return JSON.stringify(baseline) !== JSON.stringify(current);
}

test("a background key rotation alone leaves the editor clean", () => {
  const baseline = draft();
  const before = JSON.stringify(draft());
  const after = draft({ apiKey: "sk-rotated" });
  const next = adoptSyncedDraftFields(baseline, before, after);
  assert.equal(next.auth.apiKey, "sk-rotated");
  assert.equal(dirty(next, after), false);
});

test("an unrelated user edit stays unsaved across a background key sync", () => {
  const baseline = draft();
  // 用户先把站点地址改了，随后又在凭据区管理 Key：同步前的快照已经带着
  // 这次用户编辑，同步只轮换 apiKey。
  const edited = draft({ baseUrl: "https://new.example.com" });
  const before = JSON.stringify(edited);
  const after = draft({
    baseUrl: "https://new.example.com",
    apiKey: "sk-rotated",
  });
  const next = adoptSyncedDraftFields(baseline, before, after);
  assert.equal(next.auth.apiKey, "sk-rotated", "同步字段并入基线");
  assert.equal(
    next.identity.baseUrl,
    "https://old.example.com",
    "用户编辑仍是未保存修改，不能在关窗时被静默丢弃",
  );
  assert.equal(dirty(next, after), true);
});

test("a user-edited credential field is not absorbed by a sync that left it alone", () => {
  const baseline = draft();
  const edited = draft({
    accessToken: "access-typed-by-user",
    apiKey: "sk-rotated",
  });
  // 同步前用户已经手改了访问令牌；后台只轮换 apiKey，没有碰访问令牌。
  const before = JSON.stringify(
    draft({ accessToken: "access-typed-by-user" }),
  );
  const next = adoptSyncedDraftFields(baseline, before, edited);
  assert.equal(next.auth.apiKey, "sk-rotated");
  assert.equal(
    next.auth.accessToken,
    "access-old",
    "后台没改过的授权字段仍按用户编辑处理",
  );
  assert.equal(dirty(next, edited), true);
});

test("a missing or unreadable snapshot falls back to the current draft", () => {
  const baseline = draft();
  const current = draft({ baseUrl: "https://new.example.com" });
  assert.equal(
    dirty(adoptSyncedDraftFields(baseline, null, current), current),
    false,
  );
  assert.equal(
    dirty(adoptSyncedDraftFields(baseline, "{not json", current), current),
    false,
  );
});
