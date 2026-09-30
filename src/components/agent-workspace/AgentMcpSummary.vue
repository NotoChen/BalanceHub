<script setup lang="ts">
import { computed } from "vue";
import { mcpConnectionFields, mcpOptionFields, mcpTransportLabels } from "../../utils/mcp-form-fields";
import ContentEditor from "../ContentEditor.vue";
import "../../styles/modules/mcp-editor.css";
const props = defineProps<{ text: string }>();
const connection = computed<Record<string, unknown> | null>(() => {
  try { const value = JSON.parse(props.text); return value && typeof value === "object" && !Array.isArray(value) ? value : null; } catch { return null; }
});
function describe(value: unknown): string {
  if (value == null) return "未设置";
  if (typeof value !== "object") return String(value);
  if (Array.isArray(value)) return value.map(describe).join("\n");
  const object = value as Record<string, unknown>;
  if (typeof object.template === "string") return `${object.template}（由 Agent 读取环境变量）`;
  if (typeof object.grokSessionTemplate === "string") return `${object.grokSessionTemplate}（由 Grok 填入会话信息）`;
  return Object.entries(object).map(([key, content]) => `${key}：${describe(content)}`).join("\n");
}
const rows = computed(() => {
  if (!connection.value) return [];
  const value = connection.value;
  const rows = [{ label: "连接方式", value: mcpTransportLabels[String(value.type)] ?? String(value.type ?? "未识别") }];
  for (const field of mcpConnectionFields) if (value[field.key] != null) rows.push({ label: field.key === "headers" ? "认证与请求头" : field.label, value: describe(value[field.key]) });
  for (const [key, content] of Object.entries((value.connectionOptions ?? {}) as Record<string, unknown>)) rows.push({ label: mcpOptionFields.find((field) => field.key === key)?.label ?? key, value: describe(content) });
  return rows;
});
</script>
<template>
  <div class="mcp-summary">
    <dl v-if="connection" class="mcp-summary-fields"><div v-for="row in rows" :key="row.label"><dt>{{ row.label }}</dt><dd>{{ row.value }}</dd></div></dl>
    <details v-if="connection" class="mcp-extra"><summary>查看连接 JSON<span>高级</span></summary><div class="mcp-extra-body"><ContentEditor :model-value="text" :original-text="text" format="json" readonly /></div></details>
    <ContentEditor v-else :model-value="text" :original-text="text" format="json" readonly />
  </div>
</template>
