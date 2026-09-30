<script setup lang="ts">
import { computed, nextTick, ref, useId } from "vue";
import { useAgentMcpEditor, type AgentMcpEditorDraft, type AgentMcpEditorProps } from "../../composables/useAgentMcpEditor";
import type { McpAuthMode } from "../../composables/useMcpConnectionDraft";
import { fieldValue, mcpConnectionFields, type McpField } from "../../utils/mcp-form-fields";
import RadioChoiceGroup from "../RadioChoiceGroup.vue";
import ContentEditor from "../ContentEditor.vue";
import AgentMcpField from "./AgentMcpField.vue";
import "../../styles/modules/mcp-editor.css";
const props = defineProps<AgentMcpEditorProps>();
const emit = defineEmits<{ "update:modelValue": [text: string]; validation: [error: string]; draft: [value: AgentMcpEditorDraft] }>();
const { connection, mode, loading, converting, error, rawNotice, validation, firstError, setField, setTransport, change, showRaw, showForm, setRaw, useRaw } = useAgentMcpEditor(props,
  (text) => emit("update:modelValue", text), (error) => emit("validation", error), (draft) => emit("draft", draft));
const { state, schema, local, rules, retained, runtime, runtimeOptions, oauthOptions, headerOptions, tokenEnvironment, displayErrors, attempted } = connection;
const root = ref<HTMLElement>();
const tokenId = useId();
const choices = [{ value: "http", label: "远程服务", description: "填写服务提供方给出的地址" }, { value: "stdio", label: "本地程序", description: "填写安装说明中的命令和参数" }];
const authChoices: { value: McpAuthMode; label: string }[] = [
  { value: "automatic", label: "无需预填凭据" }, { value: "token", label: "访问令牌" },
  { value: "headers", label: "请求头" }, { value: "oauth", label: "OAuth 参数" }, { value: "custom", label: "组合配置" },
];
const tokenSources = [{ value: "value", label: "直接填写" }, { value: "environment", label: "从环境变量读取" }];
const transportNames: Record<string, string> = { http: "Streamable HTTP", sse: "旧版 SSE", webSocket: "WebSocket" };
const transportChoices = computed(() => (schema.value?.transports ?? []).filter((rule) => rule.remote).map((rule) => ({ value: rule.key, label: transportNames[rule.key], disabled: !rule.supported, description: rule.supportLabel })));
const transportNote = computed(() => schema.value?.transports.find((rule) => rule.key === state.value?.transport)?.supportLabel);
const primaryFields = computed(() => mcpConnectionFields.filter((field) => local.value ? ["command", "args"].includes(field.key) : field.key === "url"));
const headerField = mcpConnectionFields.find((field) => field.key === "headers")!;
const supportNote = (key: string) => rules.value.get(key)?.supportLabel ?? "";
function count(fields: McpField[], option = false) {
  return fields.filter((field) => { try { return fieldValue(field, (option ? state.value!.options : state.value!.fields)[field.key]) !== undefined; } catch { return true; } }).length;
}
function removeRetained(key: string) { if (state.value) change({ removed: [...state.value.removed, key] }); }
function chooseAuth(value: string) { change({ auth: value as McpAuthMode }); }
async function validate() {
  attempted.value = true;
  await nextTick();
  if (!validation.value) return true;
  const field = root.value?.querySelector<HTMLElement>('.mcp-field.has-error');
  let parent = field?.parentElement;
  while (parent && parent !== root.value) { if (parent instanceof HTMLDetailsElement) parent.open = true; parent = parent.parentElement; }
  field?.scrollIntoView({ block: "nearest" });
  field?.querySelector<HTMLElement>("input, textarea, button")?.focus();
  return false;
}
defineExpose({ validate });
</script>
<template>
  <section ref="root" class="mcp-editor" aria-label="MCP 连接配置">
    <header class="mcp-editor-heading"><strong>连接配置<span v-if="schema?.targetLabel" class="mcp-target"> · {{ schema.targetLabel }}</span></strong>
      <a-button v-if="mode === 'form'" type="text" size="small" :disabled="loading" @click="showRaw">编辑原文（高级）</a-button>
      <a-button v-else type="text" size="small" :loading="loading" @click="showForm">返回表单</a-button>
    </header>
    <p v-if="loading" class="mcp-help" role="status">正在读取连接字段…</p>
    <p v-if="error" class="agent-workspace-error" role="alert">{{ error }}</p>
    <template v-if="mode === 'raw'">
      <div v-if="rawNotice" class="mcp-draft-notice" role="status"><p>{{ rawNotice }}</p><a-button size="small" @click="useRaw">以当前原文为准并返回表单</a-button></div>
      <ContentEditor :model-value="modelValue" :original-text="originalText" :format="format ?? 'json'" :readonly="readonly" @update:model-value="setRaw" />
    </template>
    <fieldset v-else-if="!loading && state" class="mcp-form" :disabled="readonly">
      <section class="mcp-form-section">
        <h4>连接方式</h4>
        <RadioChoiceGroup :model-value="local ? 'stdio' : 'http'" :options="choices" label="MCP 连接方式" :disabled="readonly" class="mcp-connection-choices" @update:model-value="setTransport($event === 'stdio' ? 'stdio' : state.remoteTransport)">
          <template #default="{ option }"><strong>{{ option.label }}</strong><span>{{ option.description }}</span></template>
        </RadioChoiceGroup>
        <p v-if="state.transportChanged" class="mcp-help">保存时使用当前连接方式的字段；切回原方式可继续填写之前的草稿。</p>
        <AgentMcpField v-for="field in primaryFields" :key="field.key" :field="field" :model-value="state.fields[field.key]" :required="field.key !== 'args'" :error="displayErrors[field.key]" @update:model-value="setField(field.key, $event)" />
        <details v-if="!local" class="mcp-extra" :open="state.transport !== 'http'">
          <summary>传输方式<span>{{ transportNames[state.transport] }}</span></summary>
          <div class="mcp-extra-body"><p class="mcp-help">默认使用 Streamable HTTP。仅在服务说明明确要求 SSE 或 WebSocket 时切换。</p>
            <RadioChoiceGroup :model-value="state.transport" :options="transportChoices" label="远程传输方式" :disabled="readonly" class="mcp-inline-choices" @update:model-value="setTransport" />
            <p v-if="transportNote" class="mcp-support-note">{{ transportNote }}</p>
          </div>
        </details>
      </section>
      <section v-if="!local" class="mcp-form-section">
        <h4>认证</h4>
        <RadioChoiceGroup :model-value="state.auth" :options="authChoices" label="MCP 认证配置" :disabled="readonly" class="mcp-inline-choices" @update:model-value="chooseAuth" />
        <p v-if="state.auth === 'automatic'" class="mcp-help">无需提前填写令牌或请求头。服务需要网页登录授权时，由 Agent 发起；此选项不会禁用 Agent 的登录流程。</p>
        <p v-else-if="state.auth === 'custom'" class="mcp-help">用于现有配置包含多种认证信息，或服务明确要求组合填写的情况。避免为同一请求头重复指定来源。</p>
        <p v-if="state.auth !== 'automatic'" class="mcp-help">按服务说明选择；切换方式会保留输入草稿，保存仅包含当前方式启用的字段。</p>
        <template v-if="state.auth === 'token' || state.auth === 'custom'">
          <RadioChoiceGroup v-if="state.auth === 'token' && tokenEnvironment" :model-value="state.tokenSource" :options="tokenSources" label="访问令牌来源" :disabled="readonly" class="mcp-inline-choices" @update:model-value="change({ tokenSource: $event as 'value' | 'environment' })" />
          <div v-if="state.auth === 'custom' || state.tokenSource === 'value'" class="mcp-field" :class="{ 'has-error': displayErrors.token }">
            <header><label :for="tokenId">访问令牌<span v-if="state.auth === 'token'" class="mcp-required">必填</span></label></header>
            <p :id="`${tokenId}-help`" class="mcp-help">填写服务提供方发放的令牌。App 自动添加 Bearer 前缀；要求 X-API-Key 等名称时请选择“请求头”。</p>
            <a-input :id="tokenId" :model-value="state.token" placeholder="服务提供方发放的令牌" :aria-invalid="Boolean(displayErrors.token)" :aria-describedby="`${tokenId}-help ${tokenId}-error`" @update:model-value="change({ token: $event }, 'token')" />
            <p v-if="displayErrors.token" :id="`${tokenId}-error`" class="mcp-field-error" role="alert">{{ displayErrors.token }}</p>
          </div>
          <AgentMcpField v-if="tokenEnvironment && (state.auth === 'custom' || state.tokenSource === 'environment')" :field="tokenEnvironment" :model-value="state.options[tokenEnvironment.key]" :required="state.auth === 'token'" :error="displayErrors[tokenEnvironment.key]" :support-note="supportNote(tokenEnvironment.key)" @update:model-value="setField(tokenEnvironment.key, $event, true)" />
        </template>
        <template v-if="state.auth === 'headers' || state.auth === 'custom'">
          <AgentMcpField :field="headerField" :model-value="state.fields.headers" :error="displayErrors.headers" @update:model-value="setField('headers', $event)" />
          <details v-if="headerOptions.length" class="mcp-extra" :open="count(headerOptions, true) > 0"><summary>从环境变量或命令提供请求头<span>可选</span></summary><div class="mcp-extra-body"><AgentMcpField v-for="field in headerOptions" :key="field.key" :field="field" :model-value="state.options[field.key]" :error="displayErrors[field.key]" :support-note="supportNote(field.key)" @update:model-value="setField(field.key, $event, true)" /></div></details>
        </template>
        <template v-if="state.auth === 'oauth' || state.auth === 'custom'">
          <p class="mcp-help">仅在服务要求指定 OAuth 客户端或权限时填写。这里保存登录参数，实际登录授权仍由 Agent 完成。</p>
          <div class="mcp-option-grid"><AgentMcpField v-for="field in oauthOptions" :key="field.key" :field="field" :model-value="state.options[field.key]" :error="displayErrors[field.key]" :support-note="supportNote(field.key)" @update:model-value="setField(field.key, $event, true)" /></div>
        </template>
        <details v-if="state.auth === 'token' || state.auth === 'oauth'" class="mcp-extra" :open="count([headerField]) > 0"><summary>补充请求头<span>可选</span></summary><div class="mcp-extra-body"><AgentMcpField :field="headerField" :model-value="state.fields.headers" :error="displayErrors.headers" @update:model-value="setField('headers', $event)" /></div></details>
      </section>
      <details v-if="local && (runtime.length || runtimeOptions.length)" class="mcp-extra" :open="count(runtime) + count(runtimeOptions, true) > 0">
        <summary>运行设置<span>{{ count(runtime) + count(runtimeOptions, true) ? `已填写 ${count(runtime) + count(runtimeOptions, true)} 项` : '环境变量、工作目录等' }}</span></summary>
        <div class="mcp-extra-body"><p class="mcp-help">仅在安装说明要求时填写。启动命令不会自动安装依赖或执行网页登录。</p>
          <AgentMcpField v-for="field in runtime" :key="field.key" :field="field" :model-value="state.fields[field.key]" :error="displayErrors[field.key]" :support-note="supportNote(field.key)" @update:model-value="setField(field.key, $event)" />
          <AgentMcpField v-for="field in runtimeOptions" :key="field.key" :field="field" :model-value="state.options[field.key]" :error="displayErrors[field.key]" :support-note="supportNote(field.key)" @update:model-value="setField(field.key, $event, true)" />
        </div>
      </details>
      <details v-if="retained.length" class="mcp-extra" open><summary>已保留的其他字段<span>{{ retained.length }} 项</span></summary><div class="mcp-extra-body">
        <p class="mcp-help">这些字段来自已有配置，可能包含专属认证或运行设置。切换上方认证方式不会移除它们；确认不需要时再从草稿中移除。</p>
        <div v-for="item in retained" :key="item.field.key" class="mcp-retained-field"><AgentMcpField :field="item.field" :model-value="(item.option ? state.options : state.fields)[item.field.key]" :error="displayErrors[item.field.key]" :support-note="supportNote(item.field.key)" @update:model-value="setField(item.field.key, $event, item.option)" /><a-button type="text" size="small" status="danger" @click="removeRetained(item.field.key)">从草稿移除此字段</a-button></div>
      </div></details>
      <p v-if="converting" class="mcp-help" role="status">正在整理配置草稿…</p>
      <p v-else-if="firstError" class="mcp-help" role="status">{{ attempted ? firstError : '请补全当前连接所需的信息后保存。' }}</p>
    </fieldset>
  </section>
</template>
