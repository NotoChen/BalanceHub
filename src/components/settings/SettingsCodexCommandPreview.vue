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
    <span>命令模板</span>
    <code>{{ codexCommandPreview }}</code>
    <p v-if="settings.livenessCliKind === 'claudeCode'">
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
