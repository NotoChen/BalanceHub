<script setup lang="ts">
import { computed, onScopeDispose, ref, shallowRef, watch } from "vue";
import { useAgentCatalogStore } from "../../stores/agent-catalog";
import { useAgentMaintenanceStore } from "../../stores/agent-maintenance";
import {
  getAgentBatchOptions,
  planAgentBatch,
} from "../../api/agent-maintenance";
import { cancelAgentCatalogRead } from "../../api/agent-catalog";
import type { AgentCatalogAction } from "../../stores/agent-catalog-types";
import type { AgentBatchOption } from "../../stores/agent-maintenance-types";
import { agentCatalogActionLabels } from "../../utils/agent-catalog-display";
import { agentEnvironmentErrorMessage } from "../../stores/agent-environment";
import { withTimeout } from "../../utils/promise-timeout";
import AgentCatalogConfigurationSelector from "./AgentCatalogConfigurationSelector.vue";
const props = defineProps<{
  visible: boolean;
  assetIds: string[];
  action: AgentCatalogAction;
  workspace?: string;
}>();
const emit = defineEmits<{ close: [] }>();
const catalog = useAgentCatalogStore();
const maintenance = useAgentMaintenanceStore();
const options = shallowRef<AgentBatchOption[]>([]);
const selected = ref<Record<string, string[]>>({});
const loading = ref(false);
const error = ref("");
let revision = 0;
let readId = "";
const count = computed(() =>
  Object.values(selected.value).reduce((sum, ids) => sum + ids.length, 0),
);
function invalidate() {
  revision += 1;
  if (readId) void cancelAgentCatalogRead(readId).catch(() => {});
  readId = "";
  loading.value = false;
}
async function load() {
  invalidate();
  const request = revision;
  readId = crypto.randomUUID();
  const id = readId;
  loading.value = true;
  error.value = "";
  options.value = [];
  selected.value = {};
  try {
    await catalog.ensureReady(props.workspace);
    if (request !== revision) return;
    const result = await withTimeout(
      getAgentBatchOptions(props.assetIds, props.action, props.workspace, id),
      40_000,
      "读取批量目标超时",
    );
    if (request === revision && props.visible) options.value = result;
  } catch (failure) {
    if (request === revision)
      error.value = agentEnvironmentErrorMessage(failure);
  } finally {
    if (request === revision) {
      loading.value = false;
      void cancelAgentCatalogRead(id).catch(() => {});
      readId = "";
    }
  }
}
async function preview() {
  invalidate();
  const request = revision;
  readId = crypto.randomUUID();
  const id = readId;
  loading.value = true;
  error.value = "";
  const requests = options.value
    .filter((option) => option.plan && selected.value[option.assetId]?.length)
    .map((option) => ({
      ...option.request,
      targetIds: selected.value[option.assetId] ?? [],
    }));
  try {
    await maintenance.presentWhenReady(
      planAgentBatch(requests, props.workspace, id),
      () => request === revision && props.visible,
      90_000,
      "准备批量操作超时",
    );
    if (request === revision && props.visible) emit("close");
  } catch (failure) {
    if (request === revision)
      error.value = agentEnvironmentErrorMessage(failure);
  } finally {
    if (request === revision) {
      loading.value = false;
      void cancelAgentCatalogRead(id).catch(() => {});
      readId = "";
    }
  }
}
function select(option: AgentBatchOption, id: string, checked: boolean) {
  const current = selected.value[option.assetId] ?? [];
  selectTargets(
    option.assetId,
    checked
      ? [...new Set([...current, id])]
      : current.filter((value) => value !== id),
  );
}
function selectTargets(assetId: string, targets: string[]) {
  invalidate();
  selected.value[assetId] = [...targets];
}
watch(
  [
    () => props.visible,
    () => props.action,
    () => props.workspace,
    () => props.assetIds.join("\0"),
  ],
  () => {
    if (props.visible) void load();
    else invalidate();
  },
  { immediate: true },
);
onScopeDispose(invalidate);
</script>
<template>
  <a-modal
    :visible="visible"
    :title="`批量${agentCatalogActionLabels[action]}`"
    width="min(980px, calc(100vw - 40px))"
    :footer="false"
    closable
    mask-closable
    esc-to-close
    unmount-on-close
    @cancel="emit('close')"
  >
    <p class="agent-workspace-note">
      已选 {{ assetIds.length }} 个资产，请选择各自的目标配置。
    </p>
    <p v-if="error" class="agent-workspace-error" role="alert">
      {{ error
      }}<a-button v-if="!options.length" type="text" @click="load"
        >重试</a-button
      >
    </p>
    <div class="agent-batch-options">
      <p v-if="loading && !options.length" role="status">正在读取可用目标…</p>
      <section v-for="option in options" :key="option.assetId">
        <h3>{{ option.name }}</h3>
        <p v-if="option.reason" class="agent-workspace-note">
          {{ option.reason }}
        </p>
        <AgentCatalogConfigurationSelector
          v-if="option.plan?.selection"
          :selection="option.plan.selection"
          :selected="selected[option.assetId] ?? []"
          :can-compare="false"
          @select="selectTargets(option.assetId, $event)"
        /><template v-else-if="option.plan"
          ><label
            v-for="target in option.plan.targets"
            :key="target.targetId"
            class="batch-target"
            ><input
              type="checkbox"
              :checked="selected[option.assetId]?.includes(target.targetId)"
              :disabled="!target.available || loading"
              @change="
                select(
                  option,
                  target.targetId,
                  ($event.target as HTMLInputElement).checked,
                )
              "
            /><span
              >{{ target.label
              }}<small v-if="target.reason">{{ target.reason }}</small></span
            ></label
          >
          <p v-if="!option.plan.targets.length" class="agent-workspace-note">
            没有可操作的目标。
          </p></template
        >
      </section>
    </div>
    <footer class="batch-footer">
      <span>已选 {{ count }} 项配置</span
      ><a-button @click="emit('close')">取消</a-button
      ><a-button
        type="primary"
        :loading="loading"
        :disabled="!count"
        @click="preview"
        >预览变更</a-button
      >
    </footer>
  </a-modal>
</template>
<style scoped>
.agent-batch-options {
  max-height: 62vh;
  overflow: auto;
  padding-right: 8px;
}
.agent-batch-options > section {
  padding: 16px 0;
  border-top: 1px solid var(--color-border-2);
}
.agent-batch-options h3 {
  font-size: 15px;
  margin: 0 0 12px;
  color: var(--color-text-1);
}
.batch-target {
  display: flex;
  align-items: flex-start;
  gap: 12px;
  padding: 10px;
  color: var(--color-text-1);
}
.batch-target small {
  display: block;
  color: var(--color-text-3);
  margin-top: 4px;
}
.batch-footer {
  display: flex;
  align-items: center;
  gap: 12px;
  padding-top: 18px;
}
.batch-footer span {
  flex: 1;
  color: var(--color-text-2);
}
</style>
