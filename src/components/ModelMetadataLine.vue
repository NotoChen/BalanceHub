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
/// 站点价格由倍率或 per-token 单价换算，保留 6 位并去掉浮点尾差。
const amount = (value: number) => `$${+value.toFixed(6)}`;
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
  const group = model.priceGroup ? `（分组 ${model.priceGroup}）` : "";
  if (model.perRequest != null) {
    const source =
      model.priceSource === "site" ? `站点价格${group}` : "价格";
    return `${source}：按次计费 ${amount(model.perRequest)}/次。`;
  }
  const parts = [
    model.price.input != null ? `输入 ${amount(model.price.input)}` : "",
    model.price.output != null ? `输出 ${amount(model.price.output)}` : "",
  ].filter(Boolean);
  if (!parts.length) return "价格资料不完整，无法估算费用";
  const amounts = `${parts.join(" / ")}，每百万 token`;
  if (model.priceSource === "site") {
    return model.priceGroup
      ? `站点价格（分组 ${model.priceGroup}）：${amounts}。`
      : `站点价格：${amounts}。`;
  }
  if (model.priceSource === "manual") {
    return `卡片覆盖价格：${amounts}。`;
  }
  if (model.priceSource === "catalog") {
    return `公开参考价格：${amounts}。实际费用以中转站账单为准。`;
  }
  return `价格：${amounts}。`;
});
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
