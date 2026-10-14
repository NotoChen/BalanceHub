<script setup lang="ts">
import { onScopeDispose, ref, watch } from "vue";
import {
  listAgentSharedVersions,
  planAgentSharedVersion,
} from "../../api/agent-maintenance";
import { useAgentMaintenanceStore } from "../../stores/agent-maintenance";
import { agentEnvironmentErrorMessage } from "../../stores/agent-environment";
import { withTimeout } from "../../utils/promise-timeout";
import type { AgentSharedVersion } from "../../stores/agent-maintenance-types";
const props = defineProps<{ assetId: string; version: number }>();
const maintenance = useAgentMaintenanceStore();
const opened = ref(false);
const versions = ref<AgentSharedVersion[]>([]);
const loading = ref(false);
const error = ref("");
let revision = 0;
onScopeDispose(() => {
  revision += 1;
});
watch(
  () => [props.assetId, props.version],
  () => {
    revision += 1;
    versions.value = [];
    opened.value = false;
    loading.value = false;
    error.value = "";
  },
);
async function load(event: Event) {
  opened.value = (event.currentTarget as HTMLDetailsElement).open;
  if (!opened.value) {
    revision += 1;
    loading.value = false;
    error.value = "";
    return;
  }
  if (versions.value.length || loading.value) return;
  const request = ++revision;
  loading.value = true;
  error.value = "";
  try {
    const result = await withTimeout(
      listAgentSharedVersions(props.assetId),
      15_000,
      "读取版本历史超时",
    );
    if (request === revision) versions.value = result;
  } catch (failure) {
    if (request === revision)
      error.value = agentEnvironmentErrorMessage(failure);
  } finally {
    if (request === revision) loading.value = false;
  }
}
async function restore(version: number) {
  const request = ++revision;
  loading.value = true;
  error.value = "";
  try {
    await maintenance.presentWhenReady(
      planAgentSharedVersion(props.assetId, version),
      () => request === revision && opened.value,
      30_000,
      "比较共享版本超时",
    );
  } catch (failure) {
    if (request === revision)
      error.value = agentEnvironmentErrorMessage(failure);
  } finally {
    if (request === revision) loading.value = false;
  }
}
</script>
<template>
  <details class="agent-catalog-advanced" :open="opened" @toggle="load">
    <summary>共享版本历史 · v{{ version }}</summary>
    <p v-if="error" class="agent-workspace-error">{{ error }}</p>
    <p v-if="loading" role="status">正在读取…</p>
    <div
      v-for="item in versions"
      :key="item.version"
      class="agent-catalog-relation-row"
    >
      <span
        ><strong
          >v{{ item.version }}{{ item.current ? " · 当前版本" : "" }}</strong
        ><small
          >{{ item.fileCount }} 个文件 ·
          {{ (item.bytes / 1024).toFixed(1) }} KiB</small
        ></span
      ><a-button
        v-if="!item.current"
        size="small"
        :disabled="loading"
        @click="restore(item.version)"
        >比较并恢复</a-button
      >
    </div>
  </details>
</template>
