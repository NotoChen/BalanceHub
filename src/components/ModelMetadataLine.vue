<script setup lang="ts">
import { computed } from "vue";
import type { ModelMetadata } from "../api/model-catalog";
const props = defineProps<{ model: ModelMetadata | undefined }>();
const capacity = (value: number) =>
  value >= 1_000_000
    ? `${+(value / 1_000_000).toFixed(2)}M`
    : value >= 1000
      ? `${+(value / 1000).toFixed(1)}K`
      : String(value);
const labels = computed(() =>
  !props.model
    ? []
    : [
        ...(props.model.contextWindow
          ? [`${capacity(props.model.contextWindow)} 上下文`]
          : []),
        ...(props.model.outputLimit
          ? [`${capacity(props.model.outputLimit)} 输出`]
          : []),
        ...(props.model.reasoning ? ["推理"] : []),
        ...(props.model.inputModalities.includes("image") ? ["图像输入"] : []),
        ...(props.model.inputModalities.includes("audio") ? ["音频输入"] : []),
        ...(props.model.toolCall ? ["工具调用"] : []),
      ],
);
const price = computed(() =>
  props.model?.price.input != null && props.model.price.output != null
    ? `公开参考价格：输入 $${props.model.price.input} / 输出 $${props.model.price.output}，每百万 token。实际费用以中转站账单为准。`
    : "价格资料不完整，无法估算费用",
);
</script>
<template>
  <span
    v-if="model"
    class="model-metadata-line"
    :title="`${model.name} · ${price}`"
    ><span v-for="label in labels" :key="label">{{ label }}</span
    ><span v-if="!labels.length">{{ model.name }}</span></span
  >
</template>
<style scoped>
.model-metadata-line {
  display: flex;
  gap: 4px 12px;
  flex-wrap: wrap;
  color: var(--color-text-3);
  font-size: 11px;
  line-height: 1.6;
  font-weight: 400;
  white-space: normal;
}
</style>
