import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import { fileURLToPath } from "node:url";
import { providerProtocolLabel, providerTransportProtocol } from "../src/utils/provider-display.ts";

const headerPath = fileURLToPath(
  new URL("../src/components/provider-card/ProviderCardHeader.vue", import.meta.url),
);
const stylePath = fileURLToPath(
  new URL("../src/styles/modules/provider-card/base.css", import.meta.url),
);

test("API Key cards keep a fixed height without exposing a duplicate default-key row", () => {
  const header = readFileSync(headerPath, "utf8");
  const styles = readFileSync(stylePath, "utf8");

  assert.doesNotMatch(header, /<dt>默认 Key<\/dt>/);
  assert.match(header, /v-if="apiKeyRemark" class="provider-card-api-remark"/);
  assert.match(header, /provider-card-api-key-name/);
  assert.match(header, /ProviderApiKeySwitcher/);
  assert.match(header, /aria-label="地址"/);
  assert.match(header, /aria-label="当前 API Key"/);
  assert.match(header, /provider-card-api-protocol/);
  assert.match(header, /provider-card-api-protocol-group/);
  assert.match(header, /provider-card-api-transport/);
  assert.match(header, /provider-card-api-endpoint-text/);
  assert.match(header, /providerApiKeyLocalRemark/);
  assert.doesNotMatch(header, /replace\(\/\^https/);
  assert.doesNotMatch(header, /当前调用 · 共/);
  assert.doesNotMatch(header, /apiKeyCount\.value} 把/);
  assert.match(styles, /\.provider-card-api-key-row\s*\{[^}]*overflow:\s*visible;/s);
  assert.match(styles, /\.provider-card-api-key-value\s*\{[^}]*overflow:\s*hidden;/s);
  assert.match(
    styles,
    /\.provider-card-standard\s*\{[^}]*height:\s*312px;/s,
  );
  assert.doesNotMatch(styles, /\.provider-card-api-remark-empty/);
});

test("API Key cards expose transport and provider protocol labels independently", () => {
  assert.equal(providerTransportProtocol("https://example.test/v1"), "HTTPS");
  assert.equal(providerTransportProtocol("http://example.test"), "HTTP");
  assert.equal(providerTransportProtocol("example.test"), "");
  assert.equal(providerProtocolLabel("newApi"), "NewAPI");
  assert.equal(providerProtocolLabel("sub2Api"), "Sub2API");
  assert.equal(providerProtocolLabel("api"), "通用 API Key");
});
