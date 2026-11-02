<script setup lang="ts">
import { IconDelete, IconPlus } from "@arco-design/web-vue/es/icon";
import type { ProviderHeader } from "../../stores/providers";

/// 请求头列表编辑器。名称和值分开成两列，空名称的行在保存时由 Rust 统一
/// 校验；这里只负责维护顺序，不提前丢弃用户正在填写的半成品。
const headers = defineModel<ProviderHeader[]>({ required: true });
const props = withDefaults(
  defineProps<{
    namePlaceholder?: string;
    valuePlaceholder?: string;
    emptyNote?: string;
  }>(),
  {
    namePlaceholder: "X-Custom-Header",
    valuePlaceholder: "值",
    emptyNote: "尚未添加请求头",
  },
);

function addHeader() {
  headers.value = [...headers.value, { name: "", value: "" }];
}

function removeHeader(index: number) {
  headers.value = headers.value.filter((_, position) => position !== index);
}
</script>

<template>
  <div class="provider-header-editor">
    <div v-if="headers.length" class="provider-backup-url-list">
      <div
        v-for="(header, index) in headers"
        :key="index"
        class="provider-backup-url-row provider-header-row"
      >
        <a-input
          v-model="header.name"
          :aria-label="`请求头 ${index + 1} 名称`"
          :placeholder="props.namePlaceholder"
          allow-clear
        />
        <a-input
          v-model="header.value"
          :aria-label="`请求头 ${index + 1} 值`"
          :placeholder="props.valuePlaceholder"
          allow-clear
        />
        <a-button
          type="text"
          status="danger"
          :aria-label="`删除请求头 ${index + 1}`"
          @click="removeHeader(index)"
        >
          <template #icon><IconDelete /></template>
        </a-button>
      </div>
    </div>
    <p v-else class="provider-empty-note">{{ props.emptyNote }}</p>
    <a-button type="text" size="small" @click="addHeader">
      <template #icon><IconPlus /></template>添加请求头
    </a-button>
  </div>
</template>
