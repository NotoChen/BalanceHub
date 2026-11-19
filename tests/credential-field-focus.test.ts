import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import { fileURLToPath } from "node:url";

function source(relativePath: string) {
  return readFileSync(
    fileURLToPath(new URL(`../${relativePath}`, import.meta.url)),
    "utf8",
  );
}

test("凭据助手逐个列出缺口字段，并聚焦到对应输入框", () => {
  const assistant = source("src/components/provider-editor/ProviderCredentialAssistant.vue");

  assert.match(assistant, /const missingFieldEntries = computed/);
  assert.match(assistant, /missingCredentialRequirements\(props\.draft, schema\)/);
  assert.match(assistant, /data-credential-field/);
  assert.match(assistant, /input\.focus\(\)/);
  assert.match(assistant, /scrollIntoView\(\{ block: "center", behavior: "smooth" \}\)/);
  assert.match(assistant, /credential-completion-missing/);
  assert.match(assistant, /@click="focusCredentialField\(entry\.field\)"/);
});

test("字段红框和助手共用同一份缺口结论", () => {
  const section = source("src/components/provider-editor/ProviderEditorCredentialsSection.vue");
  const fields = source("src/components/provider-editor/ProviderCredentialFields.vue");

  assert.match(section, /const missingAuthFields = computed<string\[\]>/);
  assert.match(section, /missingCredentialRequirements\(props\.draft, schema\)/);
  assert.match(section, /:invalid-fields="missingAuthFields"/);
  assert.match(fields, /invalidFields\?: string\[\]/);
  assert.match(fields, /:data-credential-field="field\.field"/);
  assert.match(fields, /:validate-status="isInvalid\(field\) \? 'error' : undefined"/);
  assert.match(fields, /:help="isInvalid\(field\) \? '还缺这个字段' : undefined"/);
});

test("缺口标签有可点击的样式", () => {
  const styles = source("src/styles/modules/agent-settings.css");
  assert.match(styles, /\.credential-completion-missing button:hover/);
  assert.match(styles, /cursor: pointer;/);
});
