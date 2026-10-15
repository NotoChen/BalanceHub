<script setup lang="ts">
import { IconInfoCircle } from "@arco-design/web-vue/es/icon";
import type { AgentSettingReference } from "../stores/agent-settings-types";
defineProps<{ reference?: AgentSettingReference }>();
</script>

<template>
  <a-popover
    v-if="reference"
    trigger="click"
    position="bottom"
    title="对应的配置键与启动参数"
    content-class="cli-native-explanation"
  >
    <button
      type="button"
      class="cli-native-reference"
      title="查看配置说明与来源"
      aria-label="查看配置键与作用范围"
    >
      <icon-info-circle />
    </button>
    <template #content
      ><code>{{ reference.nativeKey }}</code>
      <p>{{ reference.description }}</p></template
    >
  </a-popover>
</template>
