<script setup lang="ts">
import { computed, ref, watch } from "vue";
import type { AgentCatalogComparisonDocument, AgentCatalogComparisonSide } from "../../stores/agent-catalog-types";
import type { AgentCliDescriptor } from "../../stores/provider-types";
import { agentAssetScopeLabels } from "../../composables/useAgentAssetCatalog";
import ContentDiff from "../ContentDiff.vue";
import ContentChange from "../ContentChange.vue";
import AgentCatalogComparisonDocuments from "./AgentCatalogComparisonDocuments.vue";

const props = defineProps<{ sides: AgentCatalogComparisonSide[]; agents: AgentCliDescriptor[] }>();
const selectedIds = ref<string[]>([]);
const sourceLists = computed(() => props.sides.map((side) => [
  ...(side.ownership === "managed" ? [{
    id: "definition", label: `共享库 v${side.version}`, path: null,
    documents: side.definition, complete: side.definition.length > 0, reason: side.reason,
  }] : []),
  ...side.bindings.map((binding) => ({
    id: binding.bindingId,
    label: `${props.agents.find((agent) => agent.kind === binding.agentKind)?.label ?? binding.agentKind} · ${agentAssetScopeLabels[binding.scope]}`,
    path: binding.path, documents: binding.documents,
    complete: binding.complete && binding.documents.length > 0, reason: binding.reason,
  })),
]));
watch(sourceLists, (lists) => {
  selectedIds.value = lists.map((sources, index) => sources.some((source) => source.id === selectedIds.value[index])
    ? selectedIds.value[index] : sources[0]?.id ?? "");
}, { immediate: true });
const selectedSources = computed(() => sourceLists.value.map((sources, index) => sources.find((source) => source.id === selectedIds.value[index])));
const selectionKey = computed(() => props.sides.map((side, index) => `${side.assetId}:${selectedIds.value[index]}`).join("/"));
const expandedFiles = ref(new Set<string>());
watch(selectionKey, () => { expandedFiles.value = new Set(); });
function toggleFile(key: string, event: Event) {
  if ((event.target as HTMLDetailsElement).open) expandedFiles.value.add(key);
  else expandedFiles.value.delete(key);
}
const documents = computed(() => {
  const [beforeSource, afterSource] = selectedSources.value;
  const before = new Map(beforeSource?.documents.map((document) => [document.key, document]));
  const after = new Map(afterSource?.documents.map((document) => [document.key, document]));
  const complete = beforeSource?.complete && afterSource?.complete;
  return [...new Set([...before.keys(), ...after.keys()])].map((key) => {
    const left = before.get(key);
    const right = after.get(key);
    const present = [left, right].filter((document): document is AgentCatalogComparisonDocument => !!document);
    const readable = present.every((document) => !document.truncated && document.content !== null);
    const comparable = readable && ((!!left && !!right) || !!complete);
    const permissionChanged = !!left && !!right && left.executable !== right.executable;
    const kind: keyof typeof labels = !left && complete ? "added" : !right && complete ? "removed"
      : !comparable ? "unknown" : left?.content !== right?.content || permissionChanged ? "changed" : "equal";
    const note = (!left || !right) && !complete ? "来源未完整展示，暂不判断文件是否新增或删除。" : "";
    const limit = present.some((document) => document.truncated) ? "预览已截断，暂不生成逐行差异，以免把未展示的部分误判为删除。"
      : present.some((document) => document.format === "binary") ? "二进制文件无法逐行展示；是否相同以完整内容比较结果为准。"
      : !readable ? "正文未能读取，暂不生成逐行差异。" : "";
    return { key, left, right, kind, comparable, permissionChanged, note: note || limit };
  }).sort((a, b) => Number(a.kind === "equal") - Number(b.kind === "equal")
    || Number(b.left?.path === "SKILL.md" || b.right?.path === "SKILL.md") - Number(a.left?.path === "SKILL.md" || a.right?.path === "SKILL.md"));
});
const labels = { added: "新增", removed: "删除", changed: "有修改", equal: "一致", unknown: "无法逐行比较" };
const changedCount = computed(() => documents.value.filter((document) => ["added", "removed", "changed"].includes(document.kind)).length);
const unknownCount = computed(() => documents.value.filter((document) => document.kind === "unknown").length);
</script>

<template>
  <section class="agent-catalog-relation-diff" aria-label="同名资源逐行差异">
    <div class="agent-catalog-diff-sources">
      <div v-for="(side, index) in sides" :key="side.assetId" class="agent-catalog-diff-source">
        <strong>{{ index === 0 ? '−' : '+' }} 资源 {{ index + 1 }} · {{ side.name }}</strong>
        <label v-if="sourceLists[index].length > 1">
          <span>比较来源</span>
          <select v-model="selectedIds[index]" :aria-label="`资源 ${index + 1} 的比较来源`">
            <option v-for="(source, sourceIndex) in sourceLists[index]" :key="source.id" :value="source.id">{{ sourceIndex + 1 }}. {{ source.label }}{{ source.path ? ` · ${source.path}` : '' }}</option>
          </select>
        </label>
        <span v-else>{{ selectedSources[index]?.label ?? '没有可读取的来源' }}</span>
        <code v-if="selectedSources[index]?.path">{{ selectedSources[index]?.path }}</code>
        <p v-if="selectedSources[index]?.reason" class="agent-workspace-note">{{ selectedSources[index]?.reason }}</p>
      </div>
    </div>
    <div class="agent-catalog-diff-heading"><strong>文件差异</strong><span v-if="documents.length">{{ changedCount }} 项变化<template v-if="unknownCount"> · {{ unknownCount }} 项无法逐行比较</template></span></div>
    <p v-if="!documents.length" class="agent-workspace-note">当前没有可展示的文件，请展开来源详情查看读取结果。</p>
    <div :key="selectionKey" class="agent-catalog-diff-files">
      <details v-for="document in documents" :key="document.key" class="agent-catalog-diff-file" :open="document.kind !== 'equal'" @toggle="toggleFile(document.key, $event)">
        <summary><code>{{ document.right?.path ?? document.left?.path ?? document.right?.label ?? document.left?.label }}</code><span :class="`is-${document.kind}`">{{ labels[document.kind] }}</span></summary>
        <div v-if="document.kind !== 'equal' || expandedFiles.has(document.key)" class="agent-catalog-diff-file-body">
          <p v-if="document.note" class="agent-workspace-note">{{ document.note }}</p>
          <ContentChange v-if="document.permissionChanged" label="执行权限" :before="document.left?.executable ? '可执行' : '不可执行'" :after="document.right?.executable ? '可执行' : '不可执行'" />
          <ContentDiff v-if="document.comparable" :original-text="document.left?.content ?? ''" :modified-text="document.right?.content ?? ''" :context-lines="3" />
          <div v-else class="agent-catalog-diff-originals">
            <section><strong>资源 1</strong><AgentCatalogComparisonDocuments :documents="document.left ? [document.left] : []" /></section>
            <section><strong>资源 2</strong><AgentCatalogComparisonDocuments :documents="document.right ? [document.right] : []" /></section>
          </div>
        </div>
      </details>
    </div>
  </section>
</template>
