import assert from "node:assert/strict";
import test from "node:test";

import type { Provider } from "../src/stores/provider-types.ts";
import {
  buildCcSwitchProviderDeeplink,
  canBuildCcSwitchDeeplink,
} from "../src/utils/ccswitch-deeplink.ts";

function provider(overrides: Partial<Provider> = {}): Provider {
  return {
    displayLabel: "Relay Site",
    identity: {
      id: "provider-1",
      name: "Relay Site",
      baseUrl: "https://relay.example.com/",
      protocol: "newApi",
      displayName: "Relay",
      username: "",
      userId: "",
      siteLogo: "",
      backupUrls: [],
    },
    auth: {
      mode: "apiKey",
      apiKey: "sk-test",
      apiKeyTokenId: "",
      apiKeyOptions: [],
      accessToken: "",
      sessionCookie: "",
      apiUser: "",
      loginUsername: "",
      loginPassword: "",
      refreshToken: "",
    },
    liveness: {
      useGlobal: true,
      enabled: false,
      agentBaseUrls: {
        codex: "https://openai.example.com/root/",
        claudeCode: "https://claude.example.com/root/",
      },
      intervalMode: "fixed",
      interval: 0,
      randomMinInterval: 0,
      randomMaxInterval: 0,
      timeout: 0,
      model: "",
      promptMode: "fixed",
      fixedPrompt: "",
      promptCursor: 0,
      nextAt: null,
      records: [],
      runCount: 0,
      totalInputTokens: 0,
      totalOutputTokens: 0,
      totalTokens: 0,
      totalCostUsd: 0,
    },
    ...overrides,
  } as Provider;
}

test("CC Switch OpenAI targets use the OpenAI endpoint and a single /v1 suffix", () => {
  const link = new URL(buildCcSwitchProviderDeeplink(provider(), "codex"));

  assert.equal(link.protocol, "ccswitch:");
  assert.equal(link.hostname, "v1");
  assert.equal(link.pathname, "/import");
  assert.equal(link.searchParams.get("app"), "codex");
  assert.equal(link.searchParams.get("endpoint"), "https://openai.example.com/root/v1");
  assert.equal(link.searchParams.get("apiKey"), "sk-test");
  assert.equal(link.searchParams.has("icon"), false);
});

test("CC Switch Claude target keeps the Anthropic endpoint without appending /v1", () => {
  const link = new URL(buildCcSwitchProviderDeeplink(provider(), "claude"));

  assert.equal(link.searchParams.get("endpoint"), "https://claude.example.com/root");
  assert.equal(link.searchParams.get("app"), "claude");
});

test("CC Switch import requires both a base URL and API Key", () => {
  assert.equal(canBuildCcSwitchDeeplink(provider()), true);
  assert.equal(
    canBuildCcSwitchDeeplink(
      provider({
        auth: {
          ...provider().auth,
          apiKey: "  ",
        },
      }),
    ),
    false,
  );
});

test("CC Switch uses the same card display label", () => {
  const link = new URL(buildCcSwitchProviderDeeplink(provider({ displayLabel: "Codex 备用" }), "codex"));

  assert.equal(link.searchParams.get("name"), "Codex 备用");
});
