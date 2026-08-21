import assert from "node:assert/strict";
import test from "node:test";

import type { Provider } from "../src/stores/provider-types.ts";
import { providerMatchesSearch } from "../src/utils/provider-filters.ts";

function provider(values: {
  name?: string;
  remark?: string;
  baseUrl?: string;
  username?: string;
  userId?: string;
  models?: string[];
  apiKey?: string;
  apiKeyRemarks?: string[];
  apiKeyRemoteNames?: string[];
} = {}) {
  return {
    identity: {
      name: values.name ?? "Relay Site",
      remark: values.remark ?? "",
      displayName: "Relay",
      baseUrl: values.baseUrl ?? "https://relay.example.com/v1",
      backupUrls: [],
      username: values.username ?? "alice",
      userId: values.userId ?? "user-42",
    },
    auth: {
      apiUser: "",
      apiKey: values.apiKey ?? "sk-secret",
      apiKeyOptions: (values.apiKeyRemarks ?? []).map((localName, index) => ({
        localName,
        name: values.apiKeyRemoteNames?.[index] ?? "",
        key: index === 0 ? "key-local-secret" : "key-backup-secret",
      })),
    },
    cli: { preferredModel: "" },
    liveness: {
      model: "",
      agentBaseUrls: {},
      records: [],
    },
    capabilities: { availableModels: values.models ?? ["claude-sonnet-4"] },
  } as unknown as Provider;
}

test("provider search matches visible identity, endpoint, account and model fields", () => {
  const value = provider({
    name: "North Relay",
    remark: "Claude 主用",
    baseUrl: "https://gateway.example.net/v2",
    username: "xiaoming",
    userId: "uid-9088",
    models: ["gpt-5-codex"],
  });

  assert.equal(providerMatchesSearch(value, "north"), true);
  assert.equal(providerMatchesSearch(value, "claude 主用"), true);
  assert.equal(providerMatchesSearch(value, "gateway.example.net"), true);
  assert.equal(providerMatchesSearch(value, "xiaoming"), true);
  assert.equal(providerMatchesSearch(value, "uid-9088"), true);
  assert.equal(providerMatchesSearch(value, "GPT-5-CODEX"), true);
  assert.equal(providerMatchesSearch(value, "north uid-9088"), true);
  assert.equal(providerMatchesSearch(value, "missing"), false);
});

test("provider search does not inspect credentials", () => {
  const value = provider({ apiKey: "sk-only-for-authentication" });

  assert.equal(providerMatchesSearch(value, "sk-only-for-authentication"), false);
  assert.equal(providerMatchesSearch(value, "   "), true);
});

test("provider search matches per-Key local remarks and remote names", () => {
  const value = provider({
    apiKeyRemarks: ["Codex 生产", "Claude 备用"],
    apiKeyRemoteNames: ["token-prod", "token-backup"],
  });

  assert.equal(providerMatchesSearch(value, "codex 生产"), true);
  assert.equal(providerMatchesSearch(value, "token-backup"), true);
  assert.equal(providerMatchesSearch(value, "claude token-backup"), true);
  assert.equal(providerMatchesSearch(value, "key-local-secret"), false);
});
