<script setup lang="ts">
import { computed, ref, watch, type UnwrapNestedRefs } from "vue";
import type { useAgentConfigurationEditor } from "../../composables/useAgentConfigurationEditor";
import AgentMcpEditor from "./AgentMcpEditor.vue";
import ContentEditor from "../ContentEditor.vue";
import ContextDetails from "../ContextDetails.vue";
import AgentConfigurationDiagnostics from "./AgentConfigurationDiagnostics.vue";
import AgentConfigurationPlan from "./AgentConfigurationPlan.vue";
import AgentMarkdownContent from "./AgentMarkdownContent.vue";
import { agentConfigurationFileName } from "../../utils/agent-configuration-display";
import "../../styles/modules/agent-configuration.css";
import "../../styles/modules/agent-resource-content.css";

const props = defineProps<{ model: UnwrapNestedRefs<ReturnType<typeof useAgentConfigurationEditor>>; resourceView?: boolean; startInSource?: boolean; initialDocumentId?: string | null }>();
const emit = defineEmits<{ close: [] }>();
const selectedId = ref("");
const sourceView = ref(false);
const mcpEditor = ref<InstanceType<typeof AgentMcpEditor>>();
const mcpValidation = ref<Record<string, string>>({});
const mcp = computed(() => props.resourceView && props.model.edit?.resource?.category === "mcp");
const activeDocument = computed(() => props.model.edit?.documents.find((document) => document.sourceId === selectedId.value) ?? null);
const markdown = computed(() => props.resourceView && activeDocument.value?.format === "markdown");
const writable = computed(() => props.model.edit?.documents.some((document) => !document.readOnlyReason));
watch([() => props.model.edit?.editId, () => props.initialDocumentId], () => {
  mcpValidation.value = {};
  const documents = props.model.edit?.documents ?? [];
  selectedId.value = props.initialDocumentId
    ?? (props.resourceView ? documents.find((document) => document.format === "markdown") : null)?.sourceId ?? documents[0]?.sourceId ?? "";
  sourceView.value = Boolean(props.startInSource);
}, { immediate: true });
watch(selectedId, () => { sourceView.value = Boolean(props.startInSource); });
async function prepare() {
  if (mcp.value && mcpEditor.value && !await mcpEditor.value.validate()) return;
  if (Object.values(mcpValidation.value).some(Boolean)) return;
  await props.model.prepare();
}
</script>

<template>
  <div class="agent-configuration-editor-body" :class="{ 'agent-resource-editor': resourceView }">
    <div v-if="model.pendingLabel" class="agent-configuration-draft-notice" role="status">
      <p v-if="model.submitted">当前草稿已提交保存。打开 {{ model.pendingLabel }} 将关闭这份草稿，后台任务继续执行。</p>
      <p v-else>当前内容有未保存的修改。打开 {{ model.pendingLabel }} 前，请先处理这份草稿。</p>
      <div class="agent-inline-actions"><a-button size="small" type="primary" @click="model.keepDraft">{{ model.submitted ? '继续查看当前草稿' : '继续编辑当前草稿' }}</a-button><a-button size="small" @click="model.openPending">{{ model.submitted ? '关闭草稿并打开新内容' : '放弃草稿并打开新内容' }}</a-button></div>
    </div>
    <ContextDetails v-if="resourceView && !mcp && !model.loading && !model.planVisible && model.edit?.resource && (model.edit.resource.description || model.edit.resource.facts.length)" :key="model.edit.editId" label="资源说明">
      <p v-if="model.edit.resource.description" class="agent-resource-description">{{ model.edit.resource.description }}</p>
      <dl v-if="model.edit.resource.facts.length" class="agent-resource-facts"><div v-for="fact in model.edit.resource.facts" :key="fact.label"><dt>{{ fact.label }}</dt><dd>{{ fact.value }}</dd></div></dl>
    </ContextDetails>
    <div v-if="model.loading" class="agent-resource-loading" role="status"><span>正在读取内容…</span><i /><i /><i /></div>
    <AgentConfigurationPlan v-else-if="model.planVisible && model.plan" :plan="model.plan" />
    <template v-else-if="model.edit">
      <nav v-if="model.edit.documents.length > 1" class="agent-configuration-file-tabs" aria-label="资源文件"><button v-for="document in model.edit.documents" :key="document.sourceId" type="button" :class="{ active: activeDocument?.sourceId === document.sourceId }" :aria-pressed="activeDocument?.sourceId === document.sourceId" :title="document.path" @click="selectedId = document.sourceId">{{ agentConfigurationFileName(document) }}{{ document.creating ? ' · 新建' : '' }}</button></nav>
      <section v-if="activeDocument" class="agent-configuration-document">
        <header>
          <strong v-if="resourceView">{{ markdown ? agentConfigurationFileName(activeDocument) : activeDocument.label }}</strong>
          <code v-else>{{ activeDocument.path }}</code>
          <div v-if="markdown" class="agent-resource-view-tabs" aria-label="文档视图"><button type="button" :aria-pressed="!sourceView" @click="sourceView = false">正文</button><button type="button" :aria-pressed="sourceView" @click="sourceView = true">{{ activeDocument.readOnlyReason || model.submitted ? '原文' : '编辑原文' }}</button></div>
          <span v-else-if="!mcp">{{ activeDocument.format.toUpperCase() }}{{ activeDocument.creating ? ' · 新建' : '' }}</span>
          <slot name="file-actions" :source-id="activeDocument.sourceId" />
        </header>
        <p v-if="activeDocument.readOnlyReason" class="agent-workspace-note">{{ activeDocument.readOnlyReason }}</p>
        <AgentMarkdownContent v-if="markdown && !sourceView" :text="model.textDrafts[activeDocument.sourceId] ?? ''" />
        <AgentMcpEditor v-else-if="mcp" ref="mcpEditor" :draft="model.mcpDrafts[activeDocument.sourceId]" @draft="model.setMcpDraft(activeDocument.sourceId, $event)" :key="activeDocument.sourceId" :original-text="activeDocument.originalText" :model-value="model.textDrafts[activeDocument.sourceId] ?? ''" :format="activeDocument.format" :agent-kind="model.edit.agentKind" :readonly="model.submitted || Boolean(activeDocument.readOnlyReason)" @update:model-value="model.setText(activeDocument.sourceId, $event)" @validation="mcpValidation[activeDocument.sourceId] = $event" />
        <ContentEditor v-else :key="activeDocument.sourceId" :original-text="activeDocument.originalText" :model-value="model.textDrafts[activeDocument.sourceId] ?? ''" :format="activeDocument.format" :readonly="model.submitted || Boolean(activeDocument.readOnlyReason)" @update:model-value="model.setText(activeDocument.sourceId, $event)" />
        <p v-if="resourceView" class="agent-resource-path">{{ activeDocument.path }}</p>
      </section>
      <p v-else class="agent-workspace-error" role="alert">所选文件已不可用，请返回内容重新读取。</p>
      <AgentConfigurationDiagnostics :diagnostics="model.edit.diagnostics" />
    </template>
    <p v-if="model.submitted" class="agent-workspace-note">已提交保存。查看任务结果后，可重新读取文件继续编辑。</p>
    <p v-else-if="model.editExpired" class="agent-workspace-error" role="alert">编辑会话已过期，草稿已保留。重新读取会替换当前修改。</p>
    <p v-else-if="model.planExpired" class="agent-workspace-error" role="alert">更改预览已过期，请返回编辑后重新预览。</p>
    <p v-if="model.error" class="agent-workspace-error" role="alert">{{ model.error }}</p>
    <AgentConfigurationDiagnostics :diagnostics="model.errorDiagnostics" />
    <template v-if="model.submitted"><p v-if="model.submissionError" class="agent-workspace-error" role="status">{{ model.submissionError }}</p><AgentConfigurationDiagnostics :diagnostics="model.submissionDiagnostics" /></template>
    <p v-if="model.canReload && model.hasDraft && (model.submitted || model.editExpired || model.error)" class="agent-workspace-note">重新读取成功后会替换草稿，失败时保留现有文本。</p>
    <p v-else-if="model.hasDraft && !model.submitted && !model.planVisible && !model.pendingLabel" class="agent-workspace-note">未保存的修改暂存在内存中，关闭后再点同一资源可继续编辑；退出 App 后不保留。</p>
    <footer class="agent-configuration-editor-footer">
      <a-button v-if="model.hasDraft && !model.submitted && !model.pendingLabel" type="text" @click="model.discardDraft">放弃修改</a-button>
      <a-button @click="emit('close')">关闭</a-button>
      <a-button v-if="model.canReload && !model.pendingLabel && (model.submitted || model.editExpired || model.error)" @click="model.reload">{{ model.hasDraft ? '重新读取并替换草稿' : '重新读取' }}</a-button>
      <template v-if="model.planVisible"><a-button @click="model.backToDraft">返回编辑</a-button><a-button type="primary" :disabled="!model.canConfirm" @click="model.confirm">确认保存</a-button></template>
      <template v-else-if="!model.submitted && (!resourceView || writable)"><a-button v-if="model.plan && !model.planExpired" @click="model.planVisible = true">返回预览</a-button><a-button v-if="!resourceView || !markdown || sourceView || model.hasDraft" type="primary" :loading="model.preparing" :disabled="!model.canPrepare" @click="prepare">预览更改</a-button></template>
    </footer>
  </div>
</template>
