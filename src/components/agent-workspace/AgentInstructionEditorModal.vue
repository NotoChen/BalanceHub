<script setup lang="ts">
import { computed, ref, watch } from "vue";
import { ChevronDown } from "@lucide/vue";
import type { AgentInstructionSet } from "../../api/agent-instructions";
import type { AgentCliDescriptor, AgentCliKind } from "../../stores/provider-types";
import AgentCliIcon from "../AgentCliIcon.vue";
import ContentEditor from "../ContentEditor.vue";
import FileEditorModal from "../FileEditorModal.vue";

const props = defineProps<{
  visible: boolean;
  draft: AgentInstructionSet | null;
  original?: AgentInstructionSet;
  agents: AgentCliDescriptor[];
  saving: boolean;
  error: string;
}>();
const emit = defineEmits<{
  "update:draft": [value: AgentInstructionSet];
  close: [];
  discard: [];
  save: [];
}>();
const supplementsOpen = ref(false);
const expandedAgent = ref<AgentCliKind | null>(null);
const additionCount = computed(() =>
  Object.values(props.draft?.additions ?? {}).filter((value) => value?.trim()).length,
);
watch(() => props.visible, (visible) => {
  if (!visible) return;
  supplementsOpen.value = additionCount.value > 0;
  expandedAgent.value = null;
});
function update(patch: Partial<AgentInstructionSet>) {
  if (props.draft && !props.saving) emit("update:draft", { ...props.draft, ...patch });
}
function updateAddition(kind: AgentCliKind, value: string) {
  update({ additions: { ...props.draft?.additions, [kind]: value } });
}
</script>

<template>
  <FileEditorModal :visible="visible" fill @close="emit('close')">
    <template #title>{{ draft?.id ? '编辑规则模板' : '新建规则模板' }}</template>
    <div v-if="draft" class="instruction-template-editor">
      <div class="instruction-template-scroll">
        <label class="instruction-template-name">
          <span>规则名称</span>
          <a-input
            :model-value="draft.name"
            @update:model-value="update({ name: $event })"
            placeholder="例如：日常开发规范"
            aria-label="规则名称"
            :max-length="100"
            :disabled="saving"
          />
        </label>
        <section class="instruction-common" aria-labelledby="instruction-common-title">
          <header>
            <h3 id="instruction-common-title">通用规则</h3>
            <p>应用到任一 Agent 时都会写入。例如：默认中文回复、遵守项目约定、说明验证结果。</p>
          </header>
          <ContentEditor
            :model-value="draft.common"
            @update:model-value="update({ common: $event })"
            :original-text="original?.common ?? ''"
            format="markdown"
            :readonly="saving"
            label="通用规则正文"
          />
        </section>
        <details
          class="instruction-supplements"
          :open="supplementsOpen"
          @toggle="supplementsOpen = ($event.target as HTMLDetailsElement).open"
        >
          <summary>
            <strong>Agent 专用补充</strong>
            <span>{{ additionCount ? `${additionCount} 个 Agent 已填写` : '可选' }}</span>
          </summary>
          <div v-if="supplementsOpen" class="instruction-additions">
            <p>只有个别 Agent 需要额外要求时才填写，应用时会追加在通用规则后。</p>
            <div v-for="agent in agents" :key="agent.kind" class="instruction-addition">
              <button
                type="button"
                class="instruction-addition-toggle"
                :aria-expanded="expandedAgent === agent.kind"
                :aria-controls="`instruction-addition-${agent.kind}`"
                @click="expandedAgent = expandedAgent === agent.kind ? null : agent.kind"
              >
                <AgentCliIcon :kind="agent.kind" :size="20" />
                <strong>{{ agent.label }}</strong>
                <span>{{ draft.additions[agent.kind]?.trim() ? '已填写补充' : '未填写' }}</span>
                <ChevronDown :size="16" :class="{ 'is-expanded': expandedAgent === agent.kind }" />
              </button>
              <div v-if="expandedAgent === agent.kind" :id="`instruction-addition-${agent.kind}`" class="instruction-addition-content">
                <ContentEditor
                  :model-value="draft.additions[agent.kind] ?? ''"
                  @update:model-value="updateAddition(agent.kind, $event)"
                  :original-text="original?.additions[agent.kind] ?? ''"
                  format="markdown"
                  :readonly="saving"
                  :label="`${agent.label} 专用补充`"
                />
              </div>
            </div>
            <p v-if="!agents.length">当前没有可选 Agent，可先保存通用规则。</p>
          </div>
        </details>
      </div>
      <p v-if="error" class="agent-workspace-error" role="alert">{{ error }}</p>
      <footer class="instruction-template-footer">
        <span>保存到 BalanceHub，应用后才写入 Agent 文件。</span>
        <div>
          <a-button type="text" :disabled="saving" @click="emit('discard')">放弃草稿</a-button>
          <a-button @click="emit('close')">稍后继续</a-button>
          <a-button type="primary" :loading="saving" :disabled="!draft.name.trim()" @click="emit('save')">保存模板</a-button>
        </div>
      </footer>
    </div>
  </FileEditorModal>
</template>

<style scoped>
.instruction-template-editor { display: flex; flex: 1; min-width: 0; min-height: 0; flex-direction: column; gap: 16px; }
.instruction-template-scroll { flex: 1; min-height: 0; overflow: auto; overscroll-behavior: contain; padding: 2px 6px 12px 2px; scrollbar-gutter: stable; }
.instruction-template-name { display: grid; max-width: 560px; gap: 8px; color: var(--surface-ink); font-size: 13px; font-weight: 600; }
.instruction-common { margin-top: 22px; }
.instruction-common h3 { margin: 0; color: var(--surface-ink); font-size: 14px; }
.instruction-common header p, .instruction-additions > p { margin: 6px 0 12px; color: var(--surface-ink-soft); font-size: 12px; line-height: 1.7; }
.instruction-common :deep(.content-editor) { height: clamp(280px, 42dvh, 560px); }
.instruction-supplements { margin-top: 24px; border-top: 1px solid var(--surface-line); padding-top: 18px; }
.instruction-supplements > summary { width: fit-content; max-width: 100%; color: var(--surface-ink); font-size: 13px; cursor: pointer; }
.instruction-supplements > summary > span { margin-left: 12px; color: var(--surface-ink-soft); font-size: 12px; }
.instruction-additions { padding-top: 8px; }
.instruction-addition { border-bottom: 1px solid var(--surface-line); }
.instruction-addition-toggle { display: flex; width: 100%; min-width: 0; align-items: center; gap: 10px; border: 0; border-radius: 6px; padding: 12px 10px; background: transparent; color: var(--surface-ink); font: inherit; font-size: 13px; text-align: left; cursor: pointer; }
.instruction-addition-toggle > strong { flex: 1; min-width: 0; overflow-wrap: anywhere; font-weight: 550; }
.instruction-addition-toggle > span { color: var(--surface-ink-soft); font-size: 12px; }
.instruction-addition-toggle:hover { background: var(--surface-panel-soft); }
.instruction-addition-toggle > svg { flex-shrink: 0; transition: transform 160ms ease; }
.instruction-addition-toggle .is-expanded { transform: rotate(180deg); }
.instruction-addition-content { padding: 8px 10px 18px; }
.instruction-addition-content :deep(.content-editor) { height: clamp(250px, 34dvh, 420px); }
.instruction-template-footer { display: flex; flex: 0 0 auto; flex-wrap: wrap; align-items: center; justify-content: space-between; gap: 12px; border-top: 1px solid var(--surface-line); padding-top: 14px; }
.instruction-template-footer > span { color: var(--surface-ink-soft); font-size: 12px; line-height: 1.6; }
.instruction-template-footer > div { display: flex; flex-wrap: wrap; justify-content: flex-end; gap: 8px; margin-left: auto; }
.instruction-supplements > summary:focus-visible, .instruction-addition-toggle:focus-visible { outline: 2px solid var(--surface-accent); outline-offset: 2px; border-radius: 5px; }
@media (prefers-reduced-motion: reduce) { .instruction-addition-toggle > svg { transition: none; } }
</style>
