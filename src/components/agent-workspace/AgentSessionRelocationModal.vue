<script setup lang="ts">
import { onScopeDispose, reactive, ref, watch } from "vue";
import { open } from "@tauri-apps/plugin-dialog";
import { planAgentSessionRelocation } from "../../api/agent-maintenance";
import { cancelAgentSessionQuery } from "../../api/agent-sessions";
import { useAgentMaintenanceStore } from "../../stores/agent-maintenance";
import type { AgentSessionRow } from "../../stores/agent-session-types";
import { agentEnvironmentErrorMessage } from "../../stores/agent-environment";
import { withTimeout } from "../../utils/promise-timeout";
const props = defineProps<{
  visible: boolean;
  row: AgentSessionRow | null;
  scopeRevision: string;
}>();
const emit = defineEmits<{ close: [] }>();
const maintenance = useAgentMaintenanceStore();
const draft = reactive({ destination: "", clientsClosed: false });
const busy = ref(false);
const choosing = ref(false);
const error = ref("");
const consumerId = crypto.randomUUID();
let revision = 0;
let generation = 0;
function cancel() {
  revision += 1;
  busy.value = false;
  choosing.value = false;
  if (generation)
    void cancelAgentSessionQuery({ consumerId, requestId: generation }).catch(
      () => {},
    );
}
watch(
  () => [props.visible, props.row?.sessionRef, props.scopeRevision],
  () => {
    cancel();
    error.value = "";
    draft.destination = "";
    draft.clientsClosed = false;
  },
);
onScopeDispose(cancel);
watch(() => [draft.destination, draft.clientsClosed], cancel, {
  flush: "sync",
});
async function choose() {
  if (busy.value || choosing.value) return;
  const request = revision;
  choosing.value = true;
  error.value = "";
  try {
    const path = await withTimeout(
      open({ directory: true, multiple: false, title: "选择搬迁后的项目目录" }),
      5 * 60_000,
      "目录选择已超时",
    );
    if (request === revision && typeof path === "string")
      draft.destination = path;
  } catch (failure) {
    if (request === revision)
      error.value = agentEnvironmentErrorMessage(failure);
  } finally {
    if (request === revision) choosing.value = false;
  }
}
async function preview() {
  if (
    !props.row ||
    busy.value ||
    choosing.value ||
    !draft.clientsClosed ||
    !draft.destination.trim()
  )
    return;
  const request = ++revision;
  busy.value = true;
  error.value = "";
  try {
    await maintenance.presentWhenReady(
      planAgentSessionRelocation({
        scopeRevision: props.scopeRevision,
        sessionRef: props.row.sessionRef,
        destination: draft.destination,
        consumerId,
        requestId: ++generation,
      }),
      () => request === revision && props.visible,
      65_000,
      "读取项目历史超时",
    );
    if (
      request === revision &&
      maintenance.preview?.kind === "sessionRelocation"
    )
      emit("close");
  } catch (failure) {
    if (request === revision)
      error.value = agentEnvironmentErrorMessage(failure);
  } finally {
    if (request === revision) busy.value = false;
  }
}
</script>
<template>
  <a-modal
    :visible="visible"
    title="迁移项目会话"
    width="min(720px, calc(100vw - 32px))"
    :footer="false"
    closable
    mask-closable
    esc-to-close
    unmount-on-close
    @cancel="emit('close')"
  >
    <div class="session-relocation">
      <p>项目目录搬迁或改名后，将这个项目的历史关联到新目录。</p>
      <div class="relocation-origin">
        <span>原项目目录</span><code>{{ row?.session.workdir }}</code
        ><small v-if="row?.runtimeScope.kind === 'wsl'">{{
          row.runtimeLabel
        }}</small>
      </div>
      <a-form :model="draft" layout="vertical"
        ><a-form-item label="新项目目录"
          ><div class="relocation-destination">
            <a-input
              v-model="draft.destination"
              :placeholder="
                row?.runtimeScope.kind === 'wsl'
                  ? '同一 WSL 发行版内的 Linux 绝对路径'
                  : '搬迁后已存在的项目目录'
              "
            /><a-button
              v-if="row?.runtimeScope.kind !== 'wsl'"
              :loading="choosing"
              :disabled="busy"
              @click="choose"
              >选择目录</a-button
            >
          </div></a-form-item
        ></a-form
      >
      <a-checkbox v-model="draft.clientsClosed"
        >我已关闭这个项目的 Agent 和编辑器会话</a-checkbox
      >
      <p class="agent-workspace-note">
        会一起迁移主会话、子会话及项目记忆。原始内容会另行保留，下一步可核对会话数、文件数和目录。
      </p>
      <p v-if="error" class="agent-workspace-error" role="alert">{{ error }}</p>
      <footer>
        <a-button @click="emit('close')">取消</a-button
        ><a-button
          type="primary"
          :loading="busy"
          :disabled="
            choosing || !draft.destination.trim() || !draft.clientsClosed
          "
          @click="preview"
          >预览迁移</a-button
        >
      </footer>
    </div>
  </a-modal>
</template>
<style scoped>
.session-relocation {
  display: grid;
  gap: 16px;
  color: var(--color-text-1);
}
.session-relocation > p {
  margin: 0;
  line-height: 1.7;
}
.relocation-origin {
  display: grid;
  gap: 8px;
  padding: 16px;
  background: var(--color-fill-1);
  border-radius: 8px;
}
.relocation-origin code {
  overflow-wrap: anywhere;
}
.relocation-origin span,
.relocation-origin small {
  color: var(--color-text-3);
  font-size: 12px;
}
.relocation-destination {
  display: flex;
  gap: 10px;
  width: 100%;
}
footer {
  display: flex;
  justify-content: flex-end;
  gap: 10px;
}
</style>
