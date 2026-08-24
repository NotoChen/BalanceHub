import assert from "node:assert/strict";
import test from "node:test";

import type { Provider, ProviderApiKeyOption } from "../src/stores/provider-types.ts";
import {
  providerApiKeyDisplayName,
  providerApiKeyCardName,
  providerApiKeySecondaryName,
  providerApiKeyRemark,
  providerCardTitle,
  providerDefaultApiKeyOption,
  providerUsesApiKeyOption,
} from "../src/utils/provider-display.ts";

function provider(authMode: Provider["auth"]["mode"], remark = "") {
  const displayLabel = authMode === "apiKey"
    ? remark || "Relay Site"
    : remark
      ? `Relay Site · ${remark}`
      : "Relay Site";
  return {
    displayLabel,
    identity: {
      name: "Relay Site",
      remark,
    },
    auth: {
      mode: authMode,
      apiKey: "",
      apiKeyTokenId: "",
      apiKeyOptions: [],
    },
  } as Provider;
}

function apiKeyOption(values: Partial<ProviderApiKeyOption> = {}) {
  return {
    localId: "key-local-1",
    localName: "",
    name: "",
    key: "sk-one",
    tokenId: "token-1",
    ...values,
  } as ProviderApiKeyOption;
}

test("account card title appends an optional provider remark", () => {
  assert.equal(providerCardTitle(provider("password", "Claude 主用")), "Relay Site · Claude 主用");
  assert.equal(providerCardTitle(provider("password")), "Relay Site");
});

test("API Key card exposes its remark as a standalone heading", () => {
  assert.equal(providerApiKeyRemark(provider("apiKey", "Codex 备用")), "Codex 备用");
  assert.equal(providerApiKeyRemark(provider("password", "账号卡片")), "");
});

test("API Key display keeps local remarks separate from remote names", () => {
  const remarked = apiKeyOption({ localName: "Claude 主用", name: "token-prod" });
  const remoteOnly = apiKeyOption({ localName: "", name: "token-backup" });
  const unnamed = apiKeyOption({ localName: "", name: "" });

  assert.equal(providerApiKeyDisplayName(remarked), "Claude 主用");
  assert.equal(providerApiKeyCardName(remarked), "Claude 主用");
  assert.equal(providerApiKeySecondaryName(remarked), "token-prod");
  assert.equal(providerApiKeyDisplayName(remoteOnly), "token-backup");
  assert.equal(providerApiKeyCardName(remoteOnly), "token-backup");
  assert.equal(providerApiKeySecondaryName(remoteOnly), "");
  assert.equal(providerApiKeyDisplayName(unnamed), "未命名 API Key");
  assert.equal(providerApiKeyCardName(unnamed), "");
  assert.equal(providerApiKeyCardName(apiKeyOption({ name: "当前 API Key" })), "");
  assert.equal(providerApiKeyCardName(apiKeyOption({ name: "当前配置 API Key" })), "");
});

test("default API Key resolution prefers the selected key value then token id fallback", () => {
  const value = provider("apiKey") as Provider;
  const first = apiKeyOption({ localId: "key-one", key: "sk-one", tokenId: "token-1" });
  const second = apiKeyOption({ localId: "key-two", key: "sk-two", tokenId: "token-2" });
  value.auth.apiKeyOptions = [first, second];
  value.auth.apiKey = "sk-one";
  value.auth.apiKeyTokenId = "token-2";

  assert.equal(providerDefaultApiKeyOption(value)?.localId, "key-one");

  value.auth.apiKey = "";
  assert.equal(providerDefaultApiKeyOption(value)?.localId, "key-two");
});

test("configured API Key remains identifiable when the local option cache is empty", () => {
  const value = provider("apiKey") as Provider;
  value.auth.apiKey = "sk-configured";
  const synthetic = apiKeyOption({
    localId: "",
    tokenId: "",
    key: "sk-configured",
    name: "当前配置 API Key",
  });

  assert.equal(providerDefaultApiKeyOption(value), undefined);
  assert.equal(providerUsesApiKeyOption(value, synthetic), true);
  assert.equal(providerUsesApiKeyOption(value, apiKeyOption({ key: "sk-other" })), false);
});
