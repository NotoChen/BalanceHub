<script setup lang="ts">
import type { ProviderEditorSection } from "../../composables/provider-editor-shared";
import type { ProviderEditorNavItem } from "../../composables/useProviderEditorSections";

defineProps<{
  items: ProviderEditorNavItem[];
  active: ProviderEditorSection;
}>();

const emit = defineEmits<{
  select: [section: ProviderEditorSection];
}>();
</script>

<template>
  <nav class="surface-nav provider-editor-nav" aria-label="编辑分区">
    <button
      v-for="(item, index) in items"
      :key="item.id"
      type="button"
      class="surface-nav-item"
      :class="{ 'is-active': item.id === active }"
      :aria-current="item.id === active ? 'page' : undefined"
      @click="emit('select', item.id)"
    >
      <span class="surface-nav-index" aria-hidden="true">{{ String(index + 1).padStart(2, "0") }}</span>
      <span class="surface-nav-copy">
        <strong>{{ item.title }}</strong>
        <small>{{ item.summary }}</small>
      </span>
    </button>
  </nav>
</template>
