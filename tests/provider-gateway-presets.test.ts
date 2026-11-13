import assert from "node:assert/strict";
import test from "node:test";

import {
  applyProviderGatewayPreset,
  gatewayPresetForBaseUrl,
  PROVIDER_GATEWAY_PRESETS,
} from "../src/utils/provider-gateway-presets.ts";

test("gateway presets have unique labels and addresses", () => {
  const labels = PROVIDER_GATEWAY_PRESETS.map((preset) => preset.label);
  const baseUrls = PROVIDER_GATEWAY_PRESETS.map((preset) => preset.baseUrl);
  assert.equal(new Set(labels).size, labels.length);
  assert.equal(new Set(baseUrls).size, baseUrls.length);
  for (const preset of PROVIDER_GATEWAY_PRESETS) {
    assert.match(preset.baseUrl, /^https?:\/\//);
    assert.ok(preset.label.trim());
  }
});

test("gateway preset matching ignores a trailing slash", () => {
  const preset = PROVIDER_GATEWAY_PRESETS[0];
  assert.equal(gatewayPresetForBaseUrl(`${preset.baseUrl}/`)?.label, preset.label);
  assert.equal(gatewayPresetForBaseUrl("https://unknown.example.com/v1"), undefined);
  assert.equal(gatewayPresetForBaseUrl(""), undefined);
});

test("applying a preset keeps a custom name but replaces a previous preset name", () => {
  const [first, second] = PROVIDER_GATEWAY_PRESETS;
  const identity = { name: "", baseUrl: "" };
  applyProviderGatewayPreset(identity, first);
  assert.equal(identity.baseUrl, first.baseUrl);
  assert.equal(identity.name, first.label);

  applyProviderGatewayPreset(identity, second);
  assert.equal(identity.baseUrl, second.baseUrl);
  assert.equal(identity.name, second.label);

  identity.name = "我的网关";
  applyProviderGatewayPreset(identity, first);
  assert.equal(identity.name, "我的网关");
});
