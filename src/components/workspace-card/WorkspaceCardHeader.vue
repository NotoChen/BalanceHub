<script setup lang="ts">
withDefaults(defineProps<{
  title: string;
  headingTag?: "h2" | "h3";
}>(), { headingTag: "h3" });
</script>

<template>
  <header class="workspace-card-header">
    <slot>
      <div class="workspace-card-brand">
        <div class="workspace-card-logo">
          <slot name="icon" />
        </div>
        <div class="workspace-card-brand-copy">
          <div class="workspace-card-title-row">
            <component :is="headingTag" class="workspace-card-title" :title="title"><slot name="title">{{ title }}</slot></component>
            <slot name="title-actions" />
          </div>
          <slot name="subtitle" />
        </div>
      </div>
    </slot>
    <div v-if="$slots.meta" class="workspace-card-meta">
      <slot name="meta" />
    </div>
  </header>
</template>
