<script setup lang="ts">
import { useAgentDefinitionEditor, type AgentCatalogDefinitionFormProps } from "../../composables/useAgentDefinitionEditor";
import { Trash2 } from "@lucide/vue";
import type { AgentCliKind } from "../../stores/provider-types";
import type { AgentCatalogSaveRequest } from "../../stores/agent-catalog-types";
import AgentCliIcon from "../AgentCliIcon.vue";
import { computed, ref, watch } from "vue";
import AgentMarkdownContent from "./AgentMarkdownContent.vue";
import ContentEditor from "../ContentEditor.vue";
import type { AgentMcpEditorDraft } from "../../composables/useAgentMcpEditor";
import AgentMcpEditor from "./AgentMcpEditor.vue";
import AgentDefinitionDiff from "./AgentDefinitionDiff.vue";

const props = defineProps<AgentCatalogDefinitionFormProps>();
const emit = defineEmits<{ close: []; retry: []; save: [request: AgentCatalogSaveRequest]; apply: [request: AgentCatalogSaveRequest] }>();
const { draft, originalMcpJson, editing, comparison, validationError, activeHookAgent, activeHookVariant, availableHookAgents, addHookVariant, removeHookVariant, save } = useAgentDefinitionEditor(props, (request, intent) => intent === "apply" ? emit("apply", request) : emit("save", request));
const formUnavailable = computed(() => props.loading || props.saving || (editing.value && !props.definition));
const skillSourceView = ref(Boolean(props.embedded));
const showingChanges = ref(false);
const mcpValidation = ref("");
const mcpEditor = ref<InstanceType<typeof AgentMcpEditor>>();
const mcpDraft = ref<AgentMcpEditorDraft>();
const mcpEditorKey = ref(0);
watch(() => [props.visible, props.definition, props.category, props.initialDraft], () => {
  mcpDraft.value = undefined; mcpValidation.value = ""; mcpEditorKey.value += 1;
});
async function submit(intent: "save" | "apply") {
  if (formUnavailable.value) return;
  const currentEditor = mcpEditorKey.value;
  if (draft.category === "mcp" && mcpEditor.value && !await mcpEditor.value.validate()) { validationError.value = mcpValidation.value; return; }
  if (draft.category === "mcp" && mcpValidation.value) { validationError.value = mcpValidation.value; return; }
  if (!props.visible || currentEditor !== mcpEditorKey.value) return;
  save(intent);
}
const agentLabel = (kind: AgentCliKind) => props.agents.find((agent) => agent.kind === kind)?.label || kind;
</script>

<template>
  <div :class="embedded ? 'agent-resource-definition' : 'agent-modal-body'">
    <p v-if="error || validationError" class="agent-workspace-error" role="alert">{{ error || validationError }}</p>
    <a-button v-if="error && editing && !definition && !loading" size="small" @click="emit('retry')">重新读取</a-button>
    <div v-if="loading" class="agent-workspace-empty">正在读取内容…</div>
    <form v-else class="agent-definition-form" @submit.prevent="submit('save')">
      <label><span>名称<span class="mcp-required">必填</span></span><a-input v-model="draft.name" :disabled="formUnavailable" aria-required="true" placeholder="输入便于识别的名称" :max-length="160" /></label>
      <AgentMcpEditor :readonly="formUnavailable" ref="mcpEditor" :draft="mcpDraft" @draft="mcpDraft = $event" v-if="draft.category === 'mcp'" :key="mcpEditorKey" v-model="draft.mcpJson" :original-text="originalMcpJson" @validation="mcpValidation = $event; validationError = ''" />
      <template v-else-if="draft.category === 'skill'">
        <section class="agent-configuration-document">
          <header><strong>SKILL.md</strong><div v-if="embedded" class="agent-resource-view-tabs" aria-label="文档视图"><button type="button" :aria-pressed="!skillSourceView" @click="skillSourceView = false">正文</button><button type="button" :aria-pressed="skillSourceView" @click="skillSourceView = true">编辑原文</button></div></header>
          <AgentMarkdownContent v-if="embedded && !skillSourceView" :text="draft.skillMarkdown" />
          <ContentEditor v-else :readonly="formUnavailable" v-model="draft.skillMarkdown" :original-text="definition?.skillMarkdown ?? ''" format="markdown" />
        </section>
        <p v-if="editing && definition?.files.length" class="agent-workspace-note">包内的脚本和其他文件会保留。</p>
        <details v-if="definition?.files.length" class="agent-definition-files"><summary>已收录文件 · {{ definition.files.length }}</summary><div v-for="file in definition.files" :key="file.path"><code>{{ file.path }}</code><small>{{ file.sizeBytes.toLocaleString() }} B</small></div></details>
      </template>
      <section v-else-if="draft.category === 'hook'" class="agent-hook-definition">
        <p class="agent-workspace-note">一个 Hook 可维护多个 Agent 的原生变体。分别填写事件名和规则，应用时使用对应 Agent 的变体。</p>
        <div class="agent-hook-variant-toolbar">
          <div class="agent-hook-variant-tabs" role="tablist" aria-label="Hook 的 Agent 变体">
            <button v-for="variant in draft.hookVariants" :key="variant.agentKind" type="button" role="tab" :id="`agent-hook-variant-${variant.agentKind}`" :aria-selected="activeHookAgent === variant.agentKind" aria-controls="agent-hook-variant-editor" @click="activeHookAgent = variant.agentKind"><AgentCliIcon :kind="variant.agentKind" :size="18" />{{ agentLabel(variant.agentKind) }}</button>
          </div>
          <div v-if="availableHookAgents.length" class="agent-hook-add-variants" aria-label="添加 Hook 的 Agent 变体"><a-button v-for="agent in availableHookAgents" :key="agent.kind" size="small" :disabled="formUnavailable" @click="addHookVariant(agent.kind)"><AgentCliIcon :kind="agent.kind" :size="16" />添加 {{ agent.label }} 变体</a-button></div>
        </div>
        <div v-if="activeHookVariant" id="agent-hook-variant-editor" class="agent-hook-variant-editor" role="tabpanel" :aria-labelledby="`agent-hook-variant-${activeHookVariant.agentKind}`">
          <label><span>原生事件名</span><a-input v-model="activeHookVariant.event" :disabled="formUnavailable" aria-required="true" aria-label="Hook 原生事件名" placeholder="填写此 Agent 支持的事件名称" /></label>
          <section class="agent-configuration-document"><header><strong>原生规则（JSON）</strong></header><ContentEditor label="Hook 原生规则 JSON" :readonly="formUnavailable" :key="activeHookVariant.agentKind" v-model="activeHookVariant.groupJson" :original-text="definition?.hook?.variants.find((variant) => variant.agentKind === activeHookVariant?.agentKind)?.groupJson ?? ''" format="json" /></section>
          <p class="agent-workspace-note">填写包含一个执行项的原生规则组，并保留匹配条件和其他选项。保存时校验该 Agent 的原生格式。</p>
          <p v-if="editing" class="agent-workspace-note">移除变体只更新共享版本，已应用的配置另行管理。</p>
          <a-button :disabled="formUnavailable" type="text" size="small" status="danger" :aria-label="`移除 ${agentLabel(activeHookVariant.agentKind)} 的共享变体`" @click="removeHookVariant(activeHookVariant.agentKind)"><Trash2 :size="14" />移除此变体</a-button>
        </div>
        <p v-else class="agent-workspace-empty">添加 Agent 变体后，填写其原生事件和规则。</p>
      </section>
      <p v-for="note in definition?.notes ?? []" :key="note" class="agent-workspace-note">{{ note }}</p>
      <section class="agent-definition-diff-preview">
        <a-button size="small" :aria-expanded="showingChanges" @click="showingChanges = !showingChanges">{{ showingChanges ? '收起修改差异' : '查看修改差异' }}</a-button>
        <template v-if="showingChanges"><AgentDefinitionDiff v-if="comparison.request && (draft.category !== 'mcp' || !mcpValidation)" :definition="definition" :request="comparison.request" :mcp-draft="draft.mcpJson" :agents="agents" /><p v-else class="agent-workspace-note">{{ draft.category === 'mcp' && mcpValidation ? mcpValidation : comparison.error }}</p></template>
      </section>
      <p v-if="saving" class="agent-workspace-note" role="status">正在保存本次提交的内容。关闭窗口不会取消已提交的保存。</p>
      <footer class="agent-definition-footer"><p class="agent-workspace-note">保存到共享库供以后使用；配置到 Agent 时，预览并确认后才保存和写入。</p><div class="agent-modal-actions"><a-button @click="emit('close')">取消</a-button><a-button html-type="submit" :loading="saving" :disabled="formUnavailable">保存到共享库</a-button><a-button type="primary" :disabled="formUnavailable" @click="submit('apply')">配置到 Agent…</a-button></div></footer>
    </form>
  </div>
</template>

<style scoped>
.agent-definition-diff-preview { display: grid; min-width: 0; gap: 12px; }
.agent-definition-diff-preview > .arco-btn { justify-self: start; }
</style>
