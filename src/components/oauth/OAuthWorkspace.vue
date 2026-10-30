<script setup lang="ts">
import { computed, defineAsyncComponent, ref } from "vue";
import { FileUp, ArrowUpRight, LoaderCircle } from "@lucide/vue";
import { Message } from "@arco-design/web-vue";
import { useOAuthAccountStore } from "../../stores/oauth-accounts";
import { useProviderStore } from "../../stores/providers";
import { startOAuthLogin } from "../../api/oauth-accounts";
import { oauthVendors } from "../../utils/oauth-display";
import type { OAuthVendor } from "../../stores/oauth-types";
import type { Provider } from "../../stores/provider-types";
import AgentCliIcon from "../AgentCliIcon.vue";
const AccountPanel = defineAsyncComponent(
  () => import("./OAuthAccountPanel.vue"),
);
const ImportPanel = defineAsyncComponent(
  () => import("./OAuthImportPanel.vue"),
);
const TaskPanel = defineAsyncComponent(() => import("./OAuthTaskPanel.vue"));
const GatewayPanel = defineAsyncComponent(
  () => import("./CpaGatewayPanel.vue"),
);
const emit = defineEmits<{
  edit: [provider: Provider];
  models: [provider: Provider];
}>();
const store = useOAuthAccountStore();
const providers = useProviderStore();
const starting = ref<OAuthVendor | null>(null);
const provider = computed(() =>
  providers.providers.find(
    (provider) => provider.identity.id === store.providerId,
  ),
);
const task = computed(() =>
  store.taskId ? store.tasks[store.taskId] : undefined,
);
const title = computed(() =>
  store.view === "account"
    ? provider.value?.identity.name || "账号管理"
    : store.view === "gateway"
      ? provider.value?.identity.name || "CPA 管理"
      : store.view === "import"
        ? "导入账号授权"
        : store.view === "task"
          ? "授权进度"
          : "添加 OAuth 账号",
);
async function start(vendor: OAuthVendor, id: string | null = null) {
  if (starting.value) return;
  starting.value = vendor;
  // The request returns as soon as the browser hand-off starts, so the panel
  // stays open and becomes the live progress view: waiting, device code,
  // pasted callback and saved account all happen in this one window.
  const requestedView = store.view;
  try {
    const task = await startOAuthLogin(vendor, id);
    store.accept(task);
    // A panel the user opened meanwhile is never replaced by a late answer.
    if (store.visible && store.view === requestedView) store.openTask(task.id);
  } catch (cause) {
    Message.error(String(cause));
  } finally {
    starting.value = null;
  }
}
function imported(id: string | null) {
  store.importedPreview = null;
  if (id) store.openAccount(id);
  else store.close();
}
function edit() {
  const current = provider.value;
  store.close();
  if (current) emit("edit", current);
}
function models() {
  const current = provider.value;
  store.close();
  if (current) emit("models", current);
}
</script>
<template>
  <a-modal
    :visible="store.visible"
    :title="title"
    :width="780"
    :footer="false"
    unmount-on-close
    modal-class="surface-modal oauth-workspace-modal"
    @cancel="store.close"
    @update:visible="store.visible = $event"
  >
    <div class="oauth-workspace-content">
      <template v-if="store.view === 'add'">
        <div class="oauth-vendor-grid">
          <button
            v-for="vendor in oauthVendors"
            :key="vendor.value"
            type="button"
            :disabled="starting !== null"
            :aria-busy="starting === vendor.value"
            @click="start(vendor.value)"
          >
            <AgentCliIcon :kind="vendor.value" :size="29" /><span>{{
              vendor.label
            }}</span
            ><LoaderCircle
              v-if="starting === vendor.value"
              class="oauth-vendor-spin"
              :size="16"
            /><ArrowUpRight v-else :size="16" />
          </button>
        </div>
        <p class="oauth-add-note">
          在系统浏览器完成授权，账号将作为独立卡片展示套餐、额度与模型。
        </p>
        <button
          type="button"
          class="oauth-import-button"
          @click="store.openImport()"
        >
          <FileUp :size="18" /><span>已有授权文件</span
          ><small>从其他应用或原生 CLI 导入</small><ArrowUpRight :size="15" />
        </button>
      </template>
      <AccountPanel
        v-else-if="store.view === 'account' && provider?.auth.oauth"
        :key="provider.identity.id"
        :provider="provider"
        @authorize="start(provider.auth.oauth!.vendor, provider.identity.id)"
        @models="models"
      />
      <GatewayPanel
        v-else-if="store.view === 'gateway' && provider"
        :key="provider.identity.id"
        :provider="provider"
        @edit="edit"
        @models="models"
        @import="store.openImport($event)"
      />
      <ImportPanel
        v-else-if="store.view === 'import'"
        :initial-preview="store.importedPreview"
        @imported="imported"
      />
      <TaskPanel
        v-else-if="store.view === 'task' && task"
        :key="task.id"
        :task="task"
      />
      <a-empty v-else description="账号或授权任务已不存在" />
    </div>
  </a-modal>
</template>
<style>
.oauth-workspace-modal {
  max-width: calc(100vw - 40px);
}
.oauth-workspace-modal .arco-modal-body {
  max-height: min(730px, calc(100vh - 150px));
  overflow-y: auto;
  overscroll-behavior: contain;
  padding: 20px 24px 24px;
}
.oauth-workspace-modal :is(button, summary):focus-visible {
  outline: 2px solid var(--surface-accent, rgb(var(--primary-6)));
  outline-offset: 3px;
}
</style>
<style scoped>
.oauth-vendor-grid {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: 12px;
}
.oauth-vendor-grid button {
  display: flex;
  align-items: center;
  gap: 15px;
  padding: 22px 18px;
  border: 1px solid var(--color-border-2);
  border-radius: 10px;
  background: var(--color-fill-1);
  color: var(--color-text-1);
  cursor: pointer;
  transition:
    border-color 0.18s,
    background 0.18s,
    transform 0.18s;
}
.oauth-vendor-grid button:hover {
  border-color: rgb(var(--primary-5));
  background: var(--color-primary-light-1);
  transform: translateY(-1px);
}
.oauth-vendor-grid button:disabled {
  cursor: progress;
  transform: none;
}
.oauth-vendor-grid button:disabled:not([aria-busy="true"]) {
  opacity: 0.55;
}
.oauth-vendor-grid button span {
  flex: 1;
  text-align: left;
  font-size: 15px;
  font-weight: 550;
}
.oauth-vendor-grid button > svg:last-child {
  color: var(--color-text-3);
}
.oauth-vendor-spin {
  color: rgb(var(--primary-6));
  animation: oauth-vendor-spin 1s linear infinite;
}
@keyframes oauth-vendor-spin {
  to {
    transform: rotate(360deg);
  }
}
.oauth-add-note {
  font-size: 12px;
  color: var(--color-text-3);
  line-height: 1.65;
  margin: 16px 0 24px;
}
.oauth-import-button {
  width: 100%;
  border: 1px solid var(--color-border-2);
  border-radius: 9px;
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 16px;
  background: transparent;
  cursor: pointer;
  color: var(--color-text-2);
  text-align: left;
}
.oauth-import-button small {
  margin-left: auto;
  color: var(--color-text-3);
}
.oauth-import-button:hover {
  background: var(--color-fill-1);
}
@media (prefers-reduced-motion: reduce) {
  .oauth-vendor-grid button {
    transition: none;
  }
  .oauth-vendor-spin {
    animation: none;
  }
}
@media (max-width: 550px) {
  .oauth-import-button small {
    display: none;
  }
}
</style>
