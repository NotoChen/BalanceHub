<script setup lang="ts">
import { computed, ref, watch } from "vue";
import { ChevronRight } from "@lucide/vue";
import type { AgentCatalogContent, AgentCatalogContentFile, AgentCatalogContentGroup, AgentCatalogContentSource } from "../../stores/agent-catalog-types";
import type { AgentCliDescriptor } from "../../stores/provider-types";
import { agentAssetScopeLabels } from "../../composables/useAgentAssetCatalog";
import AgentMarkdownContent from "./AgentMarkdownContent.vue";
import ContentEditor from "../ContentEditor.vue";
import AgentMcpSummary from "./AgentMcpSummary.vue";
import AgentCatalogContentDiff from "./AgentCatalogContentDiff.vue";

const props = defineProps<{ content: AgentCatalogContent | null; loading: boolean; error: string; agents: AgentCliDescriptor[] }>();
const emit = defineEmits<{ edit: [source: AgentCatalogContentSource, file: AgentCatalogContentFile]; retry: [] }>();
const sourceViews = ref<Record<string, boolean>>({});
const choosingTarget = ref(false);
const primary = computed(() => props.content?.groups[0] ?? null);
const variants = computed(() => props.content?.groups.slice(1) ?? []);
const validating = computed(() => props.loading || Boolean(props.content?.pendingSources));
const canEdit = computed(() => !validating.value && !props.error && editTargets.value.length > 0);
const readonlyReasons = computed(() => [...new Set(props.content?.groups.flatMap((group) => group.sources.flatMap((source) => source.files.flatMap((file) => file.readOnlyReason ? [file.readOnlyReason] : []))) ?? [])]);
function sourceLabel(source: AgentCatalogContentSource) {
  const agent = props.agents.find((agent) => agent.kind === source.agentKind)?.label ?? source.agentKind;
  if (source.bindingId === null) return ["共享定义", agent, props.content?.sharedVersion != null ? `v${props.content.sharedVersion}` : null].filter(Boolean).join(" · ");
  return [source.label, agent ?? "Agent", source.scope ? agentAssetScopeLabels[source.scope] : null].filter(Boolean).join(" · ");
}
const groupLabel = (group: AgentCatalogContentGroup) => [...new Set(group.sources.map(sourceLabel))].join("、");
const sourceSummary = computed(() => {
  const group = primary.value;
  if (!group) return "";
  const names = [...new Set(group.sources.map((source) => source.bindingId === null ? "共享定义"
    : props.agents.find((agent) => agent.kind === source.agentKind)?.label ?? source.agentKind ?? "原生配置"))];
  const state = validating.value ? "已读取" : !group.complete ? "尚未读全" : variants.value.length ? "对照内容"
    : group.sources.length > 1 ? "内容一致" : "内容来源";
  return `${state} · ${names.join(" / ")}${group.sources.length > 1 ? ` · ${group.sources.length} 处来源` : ""}`;
});
const editTargets = computed(() => {
  const targets = new Map<string, { id: string; source: AgentCatalogContentSource; file: AgentCatalogContentFile; documentLabel: string; labels: Set<string> }>();
  for (const group of props.content?.groups ?? []) {
    for (const source of group.sources) {
      for (const file of source.files) {
        const existing = targets.get(file.targetId);
        if (existing) {
          existing.labels.add(sourceLabel(source));
          if (existing.file.readOnlyReason && !file.readOnlyReason) { existing.source = source; existing.file = file; }
        } else {
          targets.set(file.targetId, { id: file.targetId, source, file, documentLabel: group.documents.find((document) => document.key === file.key)?.label ?? "配置文件", labels: new Set([sourceLabel(source)]) });
        }
      }
    }
  }
  return [...targets.values()].filter((target) => !target.file.readOnlyReason).map((target) => ({ ...target, label: [...target.labels].join("、") }));
});
function editTarget(id: string) {
  if (!canEdit.value) return;
  const target = editTargets.value.find((item) => item.id === id);
  if (!target) return;
  choosingTarget.value = false;
  emit("edit", target.source, target.file);
}
function edit() {
  if (!canEdit.value) return;
  if (editTargets.value.length === 1) editTarget(editTargets.value[0].id);
  else choosingTarget.value = !choosingTarget.value;
}
watch(() => props.content?.assetId, () => { sourceViews.value = {}; choosingTarget.value = false; });
</script>

<template>
  <section class="agent-content" aria-label="资源内容">
    <div v-if="loading && !primary" class="agent-resource-loading" role="status"><span>正在读取内容…</span><i /><i /><i /></div>
    <template v-else>
      <div v-if="error" role="alert"><p class="agent-workspace-error">{{ error }}</p><p v-if="primary" class="agent-workspace-note">当前保留已读到的内容，重新读取成功后可编辑。</p><a-button size="small" @click="emit('retry')">重新读取</a-button></div>
      <template v-if="primary">
        <header class="agent-content-heading"><strong>{{ primary.documents.length === 1 ? primary.documents[0]?.label : '说明与配置' }}</strong><a-button size="small" :disabled="!canEdit" :title="!editTargets.length ? readonlyReasons.join('；') : undefined" :aria-expanded="choosingTarget" @click="edit">{{ content?.category === 'mcp' ? '编辑连接' : '编辑原文' }}</a-button></header>
        <p v-if="loading" class="agent-workspace-note" role="status">{{ content?.pendingSources ? `正文已可阅读，正在核对其余 ${content.pendingSources} 个来源…` : '正在核对文件是否变化…' }}</p>
        <section v-if="choosingTarget && editTargets.length" class="agent-content-edit-targets" aria-label="选择修改文件">
          <header><strong>选择要编辑的文件</strong><a-button size="mini" type="text" @click="choosingTarget = false">取消</a-button></header>
          <div class="agent-content-file-choices">
            <button v-for="option in editTargets" :key="option.id" type="button" :disabled="!canEdit" :aria-label="`编辑 ${option.documentLabel}：${option.label}`" @click="editTarget(option.id)">
              <span><strong>{{ option.documentLabel }}</strong><small>{{ option.label }}</small><code>{{ option.file.path || '共享库中的定义' }}</code></span><ChevronRight :size="16" aria-hidden="true" />
            </button>
          </div>
          <p class="agent-workspace-note">共用同一文件的来源已合并；保存前可预览修改范围与差异。</p>
          <p v-if="readonlyReasons.length" class="agent-workspace-note">部分来源只读：{{ readonlyReasons.join('；') }}</p>
        </section>
        <details :key="content?.assetId" class="agent-content-sources">
          <summary>{{ sourceSummary }}<span>来源路径</span></summary>
          <ul><li v-for="(source, index) in primary.sources" :key="`${source.bindingId ?? 'shared'}:${index}`"><span>{{ sourceLabel(source) }}</span><code v-if="source.path">{{ source.path }}</code></li></ul>
        </details>
        <p v-if="variants.length" class="agent-content-difference-notice">{{ content?.category === 'mcp' ? '连接内容不同或尚未读全的来源列在下方。' : '其他来源的差异和读取情况列在下方。' }}</p>
        <p v-if="primary.description" class="agent-resource-description">{{ primary.description }}</p>
        <dl v-if="primary.facts.length && (content?.category !== 'mcp' || !primary.complete)" class="agent-resource-facts"><div v-for="fact in primary.facts" :key="fact.label"><dt>{{ fact.label }}</dt><dd>{{ fact.value }}</dd></div></dl>
        <p v-for="note in primary.notes" :key="note" class="agent-workspace-note">{{ note }}</p>
        <slot name="resources" />
        <p v-if="!editTargets.length && readonlyReasons.length" class="agent-workspace-note">{{ readonlyReasons.join('；') }}</p>
        <section v-for="document in primary.documents" :key="document.key" class="agent-content-document">
          <header v-if="content?.category !== 'mcp' || !primary.complete"><strong v-if="primary.documents.length > 1">{{ document.label }}</strong><span>{{ document.format.toUpperCase() }}</span><a-button v-if="document.format === 'markdown'" size="small" type="text" :aria-pressed="Boolean(sourceViews[document.key])" @click="sourceViews[document.key] = !sourceViews[document.key]">{{ sourceViews[document.key] ? '阅读正文' : '查看原文' }}</a-button></header>
          <AgentMcpSummary v-if="content?.category === 'mcp' && primary.complete" :text="document.text" />
          <AgentMarkdownContent v-else-if="document.format === 'markdown' && !sourceViews[document.key]" :text="document.text" />
          <ContentEditor v-else :original-text="document.text" :model-value="document.text" :format="document.format" readonly />
        </section>
        <section v-if="variants.length" class="agent-content-differences" aria-label="内容差异" data-catalog-focus="variants" tabindex="-1">
          <h3>{{ content?.category === 'mcp' ? '连接配置差异' : '内容差异' }}</h3>
          <section v-for="variant in variants" :key="variant.id" class="agent-content-difference">
            <header><span>− {{ groupLabel(primary) }}</span><strong>+ {{ groupLabel(variant) }}</strong></header>
            <AgentCatalogContentDiff :before="primary" :after="variant" />
          </section>
        </section>
      </template>
      <slot v-if="!primary" name="resources" />
      <section v-if="content?.unavailable.length" class="agent-content-unavailable" aria-label="未能读取的来源">
        <header><strong>{{ content.unavailable.length }} 个来源未能读取，尚未参与比较</strong><a-button size="mini" type="text" :disabled="loading" @click="emit('retry')">重新读取</a-button></header>
        <div v-for="item in content.unavailable" :key="`${item.source.bindingId ?? 'shared'}:${item.source.agentKind}`"><span>{{ sourceLabel(item.source) }}</span><p>{{ item.reason }}</p></div>
      </section>
      <p v-else-if="content && !primary" class="agent-workspace-note">没有可读取的内容，请刷新资源列表。</p>
    </template>
  </section>
</template>
