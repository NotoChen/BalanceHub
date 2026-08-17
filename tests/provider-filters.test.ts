import assert from "node:assert/strict";
import test from "node:test";

import type { Provider } from "../src/stores/provider-types.ts";
import { providerMatchesSearch } from "../src/utils/provider-filters.ts";

function provider(values: {
  name?: string;
  baseUrl?: string;
  username?: string;
  userId?: string;
  models?: string[];
  apiKey?: string;
} = {}) {
  return {
    identity: {
      name: values.name ?? "Relay Site",
      displayName: "Relay",
      baseUrl: values.baseUrl ?? "https://relay.example.com/v1",
      backupUrls: [],
      username: values.username ?? "alice",
      userId: values.userId ?? "user-42",
    },
    auth: { apiUser: "", apiKey: values.apiKey ?? "sk-secret" },
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
    baseUrl: "https://gateway.example.net/v2",
    username: "xiaoming",
    userId: "uid-9088",
    models: ["gpt-5-codex"],
  });

  assert.equal(providerMatchesSearch(value, "north"), true);
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
