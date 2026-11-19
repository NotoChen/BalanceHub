<script setup lang="ts">
import { computed } from "vue";
import type { ModelMetadata } from "../api/model-catalog";
import { modelPriceDetail, modelPriceSourceLabel, modelPriceSummary } from "../utils/model-price";
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
const price = computed(() => {
  const model = props.model;
  if (!model) return "";
  const source = modelPriceSourceLabel(model) || "价格";
  const summary = modelPriceSummary(model);
  if (!summary) {
    return modelPriceDetail(model) || "价格资料不完整，无法估算费用";
  }
  const basis =
    model.billing === "image"
      ? model.priceTiers.some((tier) => tier.perRequest != null)
        ? "，按张计价"
        : "，图片 token 单价（每百万 token）"
      : model.perRequest != null
        ? "，按次计费"
        : "，每百万 token";
  const detail = modelPriceDetail(model);
  const suffix =
    model.priceSource === "catalog" ? "实际费用以中转站账单为准。" : "";
  return `${source}：${summary}${basis}。${detail ? `${detail}。` : ""}${suffix}`;
});
</script>
<template>
  <span
    v-if="model && (labels.length || model.name !== model.id)"
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
