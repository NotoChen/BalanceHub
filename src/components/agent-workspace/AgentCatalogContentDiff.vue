<script setup lang="ts">
import { computed } from "vue";
import type { AgentCatalogContentGroup } from "../../stores/agent-catalog-types";
import ContentDiff from "../ContentDiff.vue";
import ContentChange from "../ContentChange.vue";
import ContentEditor from "../ContentEditor.vue";

const props = defineProps<{ before: AgentCatalogContentGroup; after: AgentCatalogContentGroup }>();
const documents = computed(() => (props.after.comparison?.documents ?? []).map((change) => ({
  ...change,
  before: props.before.documents.find((document) => document.key === change.key),
  after: props.after.documents.find((document) => document.key === change.key),
})));
</script>

<template>
  <div class="agent-content-comparison">
    <p v-if="!before.complete || !after.complete" class="agent-workspace-note">部分来源尚未读全，以下只比较已读取的内容。</p>
    <p v-for="note in after.notes" :key="note" class="agent-workspace-note">{{ note }}</p>
    <template v-if="after.comparison?.descriptionChanged">
      <ContentChange v-if="(before.complete && after.complete) || (before.description !== null && after.description !== null)" label="说明" :before="before.description" :after="after.description" />
      <p v-else class="agent-workspace-note">说明尚未读全，暂不判断新增或删除。</p>
    </template>
    <template v-for="fact in after.comparison?.facts ?? []" :key="fact.label">
      <ContentChange v-if="(before.complete && after.complete) || (fact.before !== null && fact.after !== null)" v-bind="fact" />
      <p v-else class="agent-workspace-note">{{ fact.label }} 尚未读全，暂不判断新增或删除。</p>
    </template>
    <section v-for="document in documents" :key="document.key" class="agent-content-document-diff">
      <header><strong>{{ document.after?.label ?? document.before?.label }}</strong><span>{{ document.before?.format.toUpperCase() ?? '无' }} → {{ document.after?.format.toUpperCase() ?? '无' }}</span></header>
      <template v-if="!document.comparable">
        <p class="agent-workspace-note">来源未完整读取，暂不判断文件的新增或删除。</p>
        <ContentEditor v-if="document.after" :original-text="document.after.text" :model-value="document.after.text" :format="document.after.format" readonly />
      </template>
      <template v-else>
        <p v-if="!document.before" class="agent-workspace-note">此来源包含额外文件。</p><p v-else-if="!document.after" class="agent-workspace-note">此来源没有这份文件。</p>
        <ContentDiff :original-text="document.before?.text ?? ''" :modified-text="document.after?.text ?? ''" :context-lines="3" />
      </template>
    </section>
    <p v-if="!documents.length && before.complete && after.complete" class="agent-workspace-note">文件原文一致。</p>
  </div>
</template>
