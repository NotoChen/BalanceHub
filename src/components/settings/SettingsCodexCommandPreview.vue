<script setup lang="ts">
import { computed } from "vue";
import type { AppSettings } from "../../stores/providers";

const props = defineProps<{
  settings: AppSettings;
}>();

const previewPrompt = computed(() =>
  props.settings.livenessPromptMode === "fixed"
    ? props.settings.livenessFixedPrompt.trim() || "Explain: ls -la"
    : props.settings.livenessPromptLibrary.find((item) => item.trim())?.trim() ||
      "Explain: ls -la",
);

const isHttp = computed(() => props.settings.livenessMethod === "http");

const httpCommandPreview = computed(() => {
  const model = props.settings.livenessModel.trim() || "<model>";
  const content = JSON.stringify(previewPrompt.value);
  switch (props.settings.livenessHttpProtocol) {
    case "anthropic":
      return [
        "POST <provider-anthropic-base-url>/v1/messages",
        "x-api-key: ***   anthropic-version: 2023-06-01",
        `{ "model": "${model}", "max_tokens": 64, "messages": [{"role":"user","content":${content}}] }`,
      ].join("\n");
    case "openaiResponses":
      return [
        "POST <provider-openai-base-url>/v1/responses",
        "Authorization: Bearer ***",
        `{ "model": "${model}", "max_output_tokens": 64, "input": ${content} }`,
      ].join("\n");
    default:
      return [
        "POST <provider-openai-base-url>/v1/chat/completions",
        "Authorization: Bearer ***",
        `{ "model": "${model}", "max_tokens": 64, "messages": [{"role":"user","content":${content}}] }`,
      ].join("\n");
  }
});

const codexCommandPreview = computed(() => {
  const cliPath =
    props.settings.livenessCliKind === "claudeCode"
      ? props.settings.claudeCliPath.trim() || "claude"
      : props.settings.codexCliPath.trim() || "codex";
  const prompt = previewPrompt.value;
  if (props.settings.livenessCliKind === "claudeCode") {
    return [
      "ANTHROPIC_API_KEY=*** \\",
      "ANTHROPIC_BASE_URL=<provider-anthropic-base-url> \\",
      "HTTPS_PROXY=<effective-proxy-if-needed> \\",
      `  ${quote(cliPath)} \\`,
      "  --bare \\",
      `  -p ${quote(prompt)} \\`,
      "  --output-format json \\",
      `  --model ${quote(props.settings.livenessModel || "claude-opus-4-8[1m]")} \\`,
      "  --max-budget-usd 0.02 \\",
      "  --no-session-persistence \\",
      "  --tools ''",
    ].join("\n");
  }
  return [
    "OPENAI_API_KEY=*** \\",
    `  ${quote(cliPath)} \\`,
    "  --ask-for-approval never \\",
    "  --sandbox read-only \\",
    "  exec \\",
    "  --skip-git-repo-check \\",
    "  --ephemeral \\",
    "  --ignore-user-config \\",
    "  --ignore-rules \\",
    "  --json \\",
    `  -m ${quote(props.settings.livenessModel || "gpt-5.5")} \\`,
    "  -c 'model_provider=\"balancehub\"' \\",
    "  -c 'model_providers.balancehub.name=\"BalanceHub\"' \\",
    "  -c 'model_providers.balancehub.base_url=\"<provider-model-base-url>\"' \\",
    "  -c 'model_providers.balancehub.wire_api=\"responses\"' \\",
    "  -c 'model_providers.balancehub.requires_openai_auth=true' \\",
    "  -o /tmp/balancehub-codex-<provider-id>-<pid>-<timestamp>.txt \\",
    `  ${quote(prompt)}`,
  ].join("\n");
});

function quote(value: string) {
  return `'${value.replace(/'/g, "'\\''")}'`;
}
</script>

<template>
  <div class="codex-command-preview">
    <span>{{ isHttp ? "请求模板" : "命令模板" }}</span>
    <code>{{ isHttp ? httpCommandPreview : codexCommandPreview }}</code>
    <p v-if="isHttp">
      运行时 <strong>API Key</strong> 注入到鉴权头（OpenAI 用 <strong>Authorization: Bearer</strong>，Anthropic 用
      <strong>x-api-key</strong>）；<strong>base_url</strong> 替换为当前中转站对应地址；按有效代理设置走 HTTP 代理。
    </p>
    <p v-else-if="settings.livenessCliKind === 'claudeCode'">
      <strong>ANTHROPIC_API_KEY</strong> 使用当前中转站 API Key 环境变量注入；
      <strong>ANTHROPIC_BASE_URL</strong> 运行时替换为当前中转站 Anthropic Base URL；
      <strong>HTTPS_PROXY</strong> 按有效代理设置注入。
    </p>
    <p v-else>
      <strong>OPENAI_API_KEY</strong> 使用当前中转站 API Key 环境变量注入；
      <strong>-m</strong> 为默认模型；<strong>base_url</strong>
      运行时替换为模型 Base URL；
      <strong>-o</strong> 使用按中转站和时间生成的临时输出文件。
    </p>
  </div>
</template>
