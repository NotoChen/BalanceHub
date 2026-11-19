<script setup lang="ts">
import { computed } from "vue";
import { useModelMetadata } from "../composables/useModelMetadata";
import ModelMetadataLine from "./ModelMetadataLine.vue";
const props = defineProps<{
  model: string;
  active: boolean;
  /// 卡片声明的模型覆盖（名称、上下文、价格）和站点价目优先于 models.dev 目录。
  providerId?: string;
  /// 换 Key 后站点价目已经失效；带上凭据版本让快照跟着重新读取。
  credentialRevision?: number;
}>();
const ids = computed(() => (props.model.trim() ? [props.model.trim()] : []));
const info = useModelMetadata(
  ids,
  computed(() => props.active),
  computed(() => props.providerId),
  computed(() => props.credentialRevision),
);
</script>
<template>
  <ModelMetadataLine :model="info.models.value.get(model.trim())" />
</template>
