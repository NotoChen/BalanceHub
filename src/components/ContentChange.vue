<script setup lang="ts">
import ContentDiff from "./ContentDiff.vue";
defineProps<{ label: string; path?: string | null; before: string | null; after: string | null }>();
</script>

<template>
  <section class="content-change">
    <header><strong>{{ label }}</strong><span v-if="before === null && after !== null">新增</span><span v-else-if="after === null && before !== null">删除</span></header>
    <code v-if="path" class="content-change-path">{{ path }}</code>
    <ContentDiff v-if="before !== null || after !== null" :original-text="before ?? ''" :modified-text="after ?? ''" />
  </section>
</template>

<style scoped>
.content-change { display: grid; min-width: 0; gap: 8px; }
.content-change > header { display: flex; flex-wrap: wrap; align-items: baseline; gap: 8px; font-size: 12px; }
.content-change > header > span, .content-change-path { color: var(--surface-muted); font-size: 11px; overflow-wrap: anywhere; }
</style>
