<script setup lang="ts">
import { computed } from "vue";
import { useModelMetadata } from "../composables/useModelMetadata";
import ModelMetadataLine from "./ModelMetadataLine.vue";
const props = defineProps<{
  model: string;
  active: boolean;
  /// 卡片声明的模型覆盖（名称、上下文、价格）优先于 models.dev 目录。
  providerId?: string;
}>();
const ids = computed(() => (props.model.trim() ? [props.model.trim()] : []));
const info = useModelMetadata(
  ids,
  computed(() => props.active),
  computed(() => props.providerId),
);
</script>
<template>
  <ModelMetadataLine :model="info.models.value.get(model.trim())" />
</template>
