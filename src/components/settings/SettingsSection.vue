<script setup lang="ts">
defineProps<{
  title: string;
  description: string;
  expanded: boolean;
}>();

defineEmits<{
  toggle: [];
}>();
</script>

<template>
  <section
    class="settings-section settings-section-card"
    :class="{ 'settings-section-expanded': expanded }"
  >
    <button type="button" class="settings-section-title" @click="$emit('toggle')">
      <span class="settings-section-copy">
        <strong>{{ title }}</strong>
        <span>{{ description }}</span>
      </span>
      <span class="settings-section-arrow">{{ expanded ? "收起" : "展开" }}</span>
    </button>
    <div v-if="expanded" class="settings-section-content">
      <slot />
    </div>
  </section>
</template>
