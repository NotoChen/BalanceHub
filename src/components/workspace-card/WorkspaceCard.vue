<script setup lang="ts">
withDefaults(defineProps<{
  fixedHeight?: boolean;
  interacting?: boolean;
}>(), { fixedHeight: true, interacting: false });
</script>

<template>
  <article
    class="workspace-card"
    :class="{ 'workspace-card-standard': fixedHeight, 'workspace-card-interacting': interacting }"
  >
    <slot name="decoration" />
    <slot name="header" />
    <div class="workspace-card-content">
      <slot />
    </div>
  </article>
</template>
