<script setup lang="ts">
import { provide, toRef } from "vue";
import { providerBoardSectionLabelKey } from "../utils/provider-board-sections";

const props = defineProps<{ label: string; count: number }>();

/// 分组标题已经写着协议（NewAPI / Sub2API / 通用 API Key），卡片头部据此
/// 不再在副标题里重复一遍。提供值放在每个分组自己的组件里，切换标题时
/// 卡片随之更新。
provide(providerBoardSectionLabelKey, toRef(props, "label"));
</script>

<template>
  <section class="provider-board-section">
    <div class="provider-board-section-header">
      <h2>{{ label }}</h2>
      <span>{{ count }}</span>
    </div>
    <slot />
  </section>
</template>
