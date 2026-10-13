<script setup lang="ts">
import { computed } from "vue";
import { useModelMetadata } from "../composables/useModelMetadata";
import ModelMetadataLine from "./ModelMetadataLine.vue";
const props = defineProps<{ model: string; active: boolean }>();
const ids = computed(() => (props.model.trim() ? [props.model.trim()] : []));
const info = useModelMetadata(
  ids,
  computed(() => props.active),
);
</script>
<template>
  <ModelMetadataLine :model="info.models.value.get(model.trim())" />
</template>
