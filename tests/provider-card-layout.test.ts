import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import { fileURLToPath } from "node:url";

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
  assert.match(
    styles,
    /\.provider-card-standard\s*\{[^}]*height:\s*312px;/s,
  );
  assert.doesNotMatch(styles, /\.provider-card-api-remark-empty/);
});
