<script setup lang="ts">
withDefaults(defineProps<{
  title: string;
  headingTag?: "h2" | "h3";
  framedIcon?: boolean;
}>(), { headingTag: "h3", framedIcon: true });
</script>

<template>
  <header class="workspace-card-header">
    <slot>
      <div class="workspace-card-brand">
        <div class="workspace-card-logo" :class="{ 'workspace-card-logo-framed': framedIcon }">
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
    <slot name="meta" />
  </header>
</template>
