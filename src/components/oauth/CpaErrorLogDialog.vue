<script setup lang="ts">
import { onScopeDispose, ref, watch } from "vue";
import type { Provider } from "../../stores/provider-types";
import type { CpaErrorLog } from "../../stores/oauth-types";
import { listCpaErrorLogs, readCpaErrorLog } from "../../api/oauth-accounts";
import { accountTimestamp } from "../../utils/oauth-display";
import { formatFileSize } from "../../utils/file-size";
const props = defineProps<{ visible: boolean; provider: Provider }>();
const emit = defineEmits<{ close: [] }>();
const loading = ref(false);
const logs = ref<CpaErrorLog[]>([]);
const content = ref<{ name: string; content: string } | null>(null);
const reading = ref(false);
const error = ref("");
/// One sequence per dialog: a close or a provider switch makes a read that is
/// still in flight stale, so a slow answer cannot land in the next session.
let sequence = 0;
let disposed = false;
onScopeDispose(() => {
  disposed = true;
  sequence++;
});
watch(
  () => props.visible,
  (visible) => {
    if (visible) void load();
    else reset();
  },
  { immediate: true },
);
watch(
  () => props.provider.identity.id,
  () => {
    if (props.visible) void load();
  },
);
async function load() {
  content.value = null;
  error.value = "";
  logs.value = [];
  loading.value = true;
  const current = ++sequence;
  try {
    const result = await listCpaErrorLogs(props.provider.identity.id);
    if (!disposed && current === sequence) logs.value = result;
  } catch (cause) {
    if (!disposed && current === sequence) error.value = String(cause);
  } finally {
    if (current === sequence) loading.value = false;
  }
}
async function open(log: CpaErrorLog) {
  if (reading.value) return;
  error.value = "";
  reading.value = true;
  const current = sequence;
  try {
    const result = await readCpaErrorLog(
      props.provider.identity.id,
      log.name,
    );
    if (!disposed && current === sequence) content.value = result;
  } catch (cause) {
    if (!disposed && current === sequence) error.value = String(cause);
  } finally {
    if (current === sequence) reading.value = false;
  }
}
function reset() {
  sequence++;
  logs.value = [];
  content.value = null;
  error.value = "";
  // A read that was in flight belongs to the closed dialog; its own sequence can
  // no longer clear these, so the close does it instead of leaving the dialog
  // permanently busy.
  loading.value = false;
  reading.value = false;
}
</script>

<template>
  <a-modal
    :visible="visible"
    :title="content ? `CPA 错误日志 · ${content.name}` : 'CPA 错误日志'"
    width="760px"
    :footer="false"
    unmount-on-close
    @cancel="emit('close')"
  >
    <a-alert v-if="error" type="warning">{{ error }}</a-alert>
    <a-spin v-if="loading || reading" tip="读取 CPA 错误日志" />
    <template v-else-if="content">
      <a-button size="mini" class="cpa-log-back" @click="content = null"
        >返回列表</a-button
      >
      <pre class="cpa-log-content">{{ content.content }}</pre>
    </template>
    <template v-else>
      <div v-if="logs.length" class="cpa-log-list">
        <button
          v-for="log in logs"
          :key="log.name"
          type="button"
          class="cpa-log-item"
          @click="open(log)"
        >
          <strong>{{ log.name }}</strong
          ><span
            >{{ formatFileSize(log.size) }} ·
            {{ accountTimestamp(log.modifiedAt) }}</span
          >
        </button>
      </div>
      <p v-else-if="!error" class="cpa-empty">CPA 未报告错误日志</p>
    </template>
  </a-modal>
</template>

<style scoped>
.cpa-empty {
  color: var(--color-text-3);
  font-size: 12px;
  margin: 0;
  line-height: 1.65;
}
.cpa-log-back {
  margin-bottom: 10px;
}
.cpa-log-list {
  display: grid;
  gap: 6px;
}
.cpa-log-item {
  display: flex;
  align-items: baseline;
  justify-content: space-between;
  gap: 14px;
  width: 100%;
  padding: 10px 12px;
  border: 1px solid var(--color-border-2);
  border-radius: 10px;
  background: var(--color-fill-1);
  text-align: left;
  cursor: pointer;
  transition:
    border-color 0.15s ease,
    background 0.15s ease;
}
.cpa-log-item:hover {
  border-color: var(--color-border-3);
  background: var(--color-fill-2);
}
.cpa-log-item strong {
  font-weight: 500;
  font-size: 13px;
  color: var(--color-text-1);
  overflow-wrap: anywhere;
}
.cpa-log-item span {
  flex-shrink: 0;
  font-size: 11px;
  color: var(--color-text-3);
}
.cpa-log-content {
  max-height: 55vh;
  margin: 0;
  padding: 13px;
  overflow: auto;
  border: 1px solid var(--color-border-2);
  border-radius: 10px;
  background: var(--color-fill-1);
  color: var(--color-text-2);
  font-family: ui-monospace, SFMono-Regular, Menlo, Consolas, monospace;
  font-size: 11.5px;
  line-height: 1.65;
  white-space: pre-wrap;
  overflow-wrap: anywhere;
}
</style>
