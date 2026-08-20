<script setup lang="ts">
import { computed, ref } from "vue";
import {
  IconDelete,
  IconFolder,
  IconHome,
  IconLaunch,
  IconRefresh,
  IconRight,
  IconUp,
} from "@arco-design/web-vue/es/icon";
import {
  type AgentCliKind,
  type CliSessionIndexState,
  type CliSessionSearchResult,
  type Provider,
  type ProviderApiKeyOption,
  type TemporaryCliSessionMode,
  type TemporaryCliTerminalKind,
  type Workspace,
  type WorkspaceDirectoryListing,
} from "../stores/providers";
import { useCliRuntimeStore } from "../stores/cli-runtime";
import type { SelectOption } from "../utils/liveness-options";
import { agentCliLabel, agentCliTool } from "../utils/cli-environment";
import {
  effectiveProviderApiKeyOptions,
  isProviderApiKeyUsable,
} from "../utils/provider-api-key-options.ts";
import CliIconSelector from "./CliIconSelector.vue";
import ProviderAuthIcon from "./ProviderAuthIcon.vue";
import TerminalIconSelector from "./TerminalIconSelector.vue";
import WorkspaceSessionHistoryPanel from "./WorkspaceSessionHistoryPanel.vue";

const props = defineProps<{
  visible: boolean;
  provider: Provider | null;
  cliKind: AgentCliKind;
  cliOptions: SelectOption<AgentCliKind>[];
  apiKeys: ProviderApiKeyOption[];
  apiKeyLoading: boolean;
  apiKeyError: string;
  apiKeyLocalId: string;
  selectedModel: string;
  sessionName: string;
  canNameSession: boolean;
  sessionMode: TemporaryCliSessionMode;
  selectedResumeId: string;
  terminalKind: TemporaryCliTerminalKind;
  terminalOptions: SelectOption<TemporaryCliTerminalKind>[];
  workspaces: Workspace[];
  directory: WorkspaceDirectoryListing | null;
  pathDraft: string;
  browsing: boolean;
  launchPreviewVisible: boolean;
  launchPreviewLoading: boolean;
  forgettingPath: string | null;
  error: string;
  historyQuery: string;
  historyResults: CliSessionSearchResult[];
  historyLoading: boolean;
  historyError: string;
  historyIndexState: CliSessionIndexState;
  historyIndexMessage: string;
  selectedSessionTitle: string;
}>();

const emit = defineEmits<{
  "update:visible": [visible: boolean];
  "update:pathDraft": [path: string];
  "update:cliKind": [kind: AgentCliKind];
  "update:apiKeyLocalId": [localId: string];
  "update:selectedModel": [model: string];
  "update:sessionName": [name: string];
  "update:sessionMode": [mode: TemporaryCliSessionMode];
  "update:selectedResumeId": [id: string];
  "update:terminalKind": [kind: TemporaryCliTerminalKind];
  "update:historyQuery": [query: string];
  browse: [path?: string];
  launch: [path?: string];
  forget: [path: string];
  "view-session": [session: CliSessionSearchResult["session"]];
  "refresh-sessions": [path?: string];
}>();

const showHidden = ref(false);
const store = useCliRuntimeStore();

const pathModel = computed({
  get: () => props.pathDraft,
  set: (value: string) => emit("update:pathDraft", value),
});

const modalTitle = computed(() =>
  props.provider ? `${props.provider.identity.name} · 启动临时 CLI` : "启动临时 CLI",
);

const orderedWorkspaces = computed(() => {
  return [...props.workspaces].sort(
    (left, right) => right.useCount - left.useCount || left.path.localeCompare(right.path),
  );
});

const visibleDirectories = computed(() =>
  (props.directory?.entries ?? []).filter((entry) => showHidden.value || !entry.hidden),
);

const launchLocked = computed(
  () => props.launchPreviewVisible || props.launchPreviewLoading,
);
const cliLabel = computed(() => agentCliLabel(store.cliEnvironmentProbe, props.cliKind));
const selectedCliTool = computed(() => agentCliTool(store.cliEnvironmentProbe, props.cliKind));
const supportsModelSelection = computed(
  () => selectedCliTool.value?.capabilities.modelSelection ?? false,
);
const supportsSessionHistory = computed(
  () =>
    Boolean(selectedCliTool.value?.capabilities.sessionHistory)
    && Boolean(selectedCliTool.value?.capabilities.sessionSearch)
    && Boolean(selectedCliTool.value?.capabilities.sessionDetail)
    && Boolean(selectedCliTool.value?.capabilities.sessionResume),
);
const sessionNamingHint = computed(() => {
  if (props.sessionMode !== "new" || props.canNameSession) return "";
  return selectedCliTool.value?.sessionNameHint || `${cliLabel.value} 当前不支持启动前命名`;
});
const preferredModel = computed(() => props.provider?.cli.preferredModel?.trim() || "");
const fixedModel = computed(() => (props.sessionMode === "new" ? preferredModel.value : ""));
const modelPlaceholder = computed(() =>
  props.sessionMode === "new" ? "选择或输入模型（可选）" : "不指定则沿用原会话模型",
);
const historyQueryModel = computed({
  get: () => props.historyQuery,
  set: (value: string) => emit("update:historyQuery", value),
});
const historySelectionMissing = computed(
  () => props.sessionMode === "history" && !props.selectedResumeId,
);

const modelOptions = computed(() => {
  const models = props.provider?.capabilities.availableModels ?? [];
  return [...new Set(models.map((model) => model.trim()).filter(Boolean))].sort((left, right) =>
    left.localeCompare(right),
  );
});
const effectiveApiKeys = computed(() => {
  const providerKey = props.provider?.auth.apiKey.trim() || "";
  return effectiveProviderApiKeyOptions(providerKey, props.apiKeys);
});
const usableApiKeys = computed(() => effectiveApiKeys.value.filter(isProviderApiKeyUsable));
const hasSingleApiKey = computed(() => usableApiKeys.value.length === 1);
const singleApiKey = computed(() => usableApiKeys.value[0] ?? null);
const cliKindModel = computed({
  get: () => props.cliKind,
  set: (value: AgentCliKind) => emit("update:cliKind", value),
});
const apiKeyModel = computed({
  get: () => props.apiKeyLocalId,
  set: (value: string) => emit("update:apiKeyLocalId", value),
});
const selectedModelModel = computed({
  get: () => props.selectedModel,
  set: (value: string) => emit("update:selectedModel", value),
});
const sessionNameModel = computed({
  get: () => props.sessionName,
  set: (value: string) => emit("update:sessionName", value),
});
const sessionModeModel = computed({
  get: () => props.sessionMode,
  set: (value: TemporaryCliSessionMode) => emit("update:sessionMode", value),
});
const terminalKindModel = computed({
  get: () => props.terminalKind,
  set: (value: TemporaryCliTerminalKind) => emit("update:terminalKind", value),
});
function workspaceName(path: string) {
  const normalized = path.replace(/[\\/]+$/, "");
  return normalized.split(/[\\/]/).pop() || path;
}

function browseDraftPath() {
  const path = pathModel.value.trim();
  if (path) {
    emit("browse", path);
  }
}

function handleVisibleChange(visible: boolean) {
  emit("update:visible", visible);
}

</script>

<template>
  <a-modal
    :visible="visible"
    width="min(940px, calc(100vw - 32px))"
    modal-class="surface-modal workspace-picker-modal"
    title-align="start"
    :footer="false"
    closable
    mask-closable
    esc-to-close
    unmount-on-close
    @update:visible="handleVisibleChange"
  >
    <template #title>
      <div class="surface-modal-title workspace-picker-title">
        <span class="surface-modal-title-icon"><icon-folder /></span>
        <span class="surface-modal-title-copy">
          <strong>{{ modalTitle }}</strong>
        </span>
      </div>
    </template>

    <div class="workspace-picker">
      <aside class="workspace-history">
        <div class="workspace-history-header">
          <strong>常用工作空间</strong>
        </div>

        <div v-if="orderedWorkspaces.length === 0" class="workspace-history-empty">
          <icon-folder />
          <strong>暂无工作空间</strong>
        </div>

        <div v-else class="workspace-history-list">
          <div
            v-for="workspace in orderedWorkspaces"
            :key="workspace.path"
            class="workspace-history-item"
          >
            <button
              type="button"
              class="workspace-history-launch"
              :class="{ selected: directory?.currentPath === workspace.path }"
              :title="`选择工作空间：${workspace.path}`"
              :disabled="launchLocked"
              @click="emit('browse', workspace.path)"
            >
              <span class="workspace-history-copy">
                <strong>{{ workspaceName(workspace.path) }}</strong>
              </span>
            </button>
            <div class="workspace-history-actions">
              <a-tooltip content="移除工作空间记录">
                <a-button
                  class="workspace-history-remove"
                  type="text"
                  shape="circle"
                  size="mini"
                  status="danger"
                  :loading="forgettingPath === workspace.path"
                  :disabled="launchLocked"
                  aria-label="移除工作空间记录"
                  @click="emit('forget', workspace.path)"
                >
                  <template #icon><icon-delete /></template>
                </a-button>
              </a-tooltip>
            </div>
          </div>
        </div>

        <section
          class="workspace-launch-config workspace-history-launch-config workspace-terminal-launch-config"
          aria-label="终端选择"
        >
          <div class="workspace-launch-config-field workspace-launch-terminal-field">
            <span class="workspace-config-label">终端</span>
            <TerminalIconSelector
              v-model="terminalKindModel"
              :options="terminalOptions"
              :disabled="launchLocked"
            />
          </div>
        </section>
      </aside>

      <section
        class="workspace-browser"
        :class="{ 'workspace-browser-history-mode': sessionMode === 'history' }"
      >
        <section class="workspace-launch-config workspace-ai-config" aria-label="AI 启动配置">
          <div class="workspace-launch-config-kind">
            <span class="workspace-config-label">CLI</span>
            <CliIconSelector
              v-model="cliKindModel"
              :options="cliOptions"
              :disabled="launchLocked"
            />
          </div>
          <div class="workspace-launch-config-field">
            <span class="workspace-config-label">API Key</span>
            <div v-if="hasSingleApiKey && singleApiKey" class="workspace-fixed-credential">
              <ProviderAuthIcon mode="apiKey" />
              <span :title="singleApiKey.localName || singleApiKey.name || '当前配置 API Key'">
                {{ singleApiKey.localName || singleApiKey.name || "当前配置 API Key" }}
              </span>
            </div>
            <a-select
              v-else
              v-model="apiKeyModel"
              size="small"
              :loading="apiKeyLoading"
              :disabled="launchLocked"
              placeholder="选择 API Key"
            >
              <a-option
                v-for="option in effectiveApiKeys"
                :key="option.localId || option.tokenId || option.key"
                :value="option.localId || option.tokenId || option.key"
                :disabled="!isProviderApiKeyUsable(option)"
              >
                {{ option.localName || option.name || "未命名 API Key" }}
                <span v-if="!isProviderApiKeyUsable(option)" class="workspace-api-key-unavailable">（不可用）</span>
              </a-option>
            </a-select>
          </div>
          <div
            v-if="supportsModelSelection"
            class="workspace-launch-config-field workspace-launch-model-field"
          >
            <span class="workspace-config-label">模型</span>
            <div v-if="fixedModel" class="workspace-fixed-credential workspace-fixed-model">
              <span :title="fixedModel">{{ fixedModel }}</span>
            </div>
            <a-select
              v-else
              v-model="selectedModelModel"
              size="small"
              allow-search
              allow-clear
              allow-create
              :disabled="launchLocked"
              :placeholder="modelPlaceholder"
            >
              <a-option v-for="model in modelOptions" :key="model" :value="model">
                {{ model }}
              </a-option>
            </a-select>
          </div>
        </section>
        <section class="workspace-session-picker" aria-label="会话启动方式">
          <div class="workspace-session-header">
            <span class="workspace-config-label">会话</span>
            <a-radio-group v-model="sessionModeModel" type="button" size="small" :disabled="launchLocked">
              <a-radio value="new">新会话</a-radio>
              <a-radio v-if="supportsSessionHistory" value="history">继续历史会话</a-radio>
            </a-radio-group>
          </div>
          <div v-if="canNameSession" class="workspace-session-name-field">
            <span class="workspace-config-label">会话名称</span>
            <a-input
              v-model="sessionNameModel"
              size="small"
              allow-clear
              :disabled="launchLocked"
              placeholder="可选，例如：支付模块重构"
            />
          </div>
          <a-alert
            v-else-if="sessionNamingHint"
            class="workspace-session-capability-note"
            type="info"
            show-icon
          >
            {{ sessionNamingHint }}
          </a-alert>
          <WorkspaceSessionHistoryPanel
            v-if="supportsSessionHistory && sessionMode !== 'new'"
            v-model:query="historyQueryModel"
            :results="historyResults"
            :loading="historyLoading"
            :error="historyError"
            :index-state="historyIndexState"
            :index-message="historyIndexMessage"
            :selected-resume-id="selectedResumeId"
            :selected-session-title="selectedSessionTitle"
            :workdir="directory?.currentPath || ''"
            :disabled="launchLocked"
            @view-session="emit('view-session', $event)"
            @refresh="emit('refresh-sessions', $event)"
          />
        </section>
        <a-alert v-if="apiKeyError" type="warning">
          API Key 列表读取失败：{{ apiKeyError }}
        </a-alert>
        <a-alert v-if="error" type="error" show-icon>{{ error }}</a-alert>
        <template v-if="sessionMode === 'new'">
          <div class="workspace-browser-toolbar">
            <div class="workspace-browser-navigation">
              <a-tooltip content="主目录">
                <a-button
                  shape="circle"
                  :disabled="browsing || launchLocked || !directory"
                  aria-label="主目录"
                  @click="emit('browse', directory?.homePath)"
                >
                  <template #icon><icon-home /></template>
                </a-button>
              </a-tooltip>
              <a-tooltip content="上级目录">
                <a-button
                  shape="circle"
                  :disabled="browsing || launchLocked || !directory?.parentPath"
                  aria-label="上级目录"
                  @click="emit('browse', directory?.parentPath ?? undefined)"
                >
                  <template #icon><icon-up /></template>
                </a-button>
              </a-tooltip>
              <a-tooltip content="刷新目录">
                <a-button
                  shape="circle"
                  :loading="browsing"
                  :disabled="launchLocked || !directory"
                  aria-label="刷新目录"
                  @click="emit('browse', directory?.currentPath)"
                >
                  <template #icon><icon-refresh /></template>
                </a-button>
              </a-tooltip>
            </div>
            <a-checkbox v-model="showHidden">显示隐藏目录</a-checkbox>
          </div>

          <div class="workspace-path-row">
            <a-input
              v-model="pathModel"
              :disabled="launchLocked"
              placeholder="输入工作空间路径"
              @keyup.enter="browseDraftPath"
            >
              <template #prefix><icon-folder /></template>
            </a-input>
            <a-tooltip content="打开路径">
              <a-button
                type="primary"
                shape="circle"
                :disabled="browsing || launchLocked || !pathModel.trim()"
                aria-label="打开路径"
                @click="browseDraftPath"
              >
                <template #icon><icon-right /></template>
              </a-button>
            </a-tooltip>
          </div>

          <div class="workspace-directory-scroll">
            <a-spin class="workspace-directory-spin" :loading="browsing">
              <div v-if="directory && visibleDirectories.length > 0" class="workspace-directory-list">
                <button
                  v-for="entry in visibleDirectories"
                  :key="entry.path"
                  type="button"
                  class="workspace-directory-item"
                  :disabled="browsing || launchLocked"
                  :title="entry.path"
                  @click="emit('browse', entry.path)"
                >
                  <icon-folder />
                  <span>{{ entry.name }}</span>
                  <icon-right />
                </button>
              </div>
              <div v-else-if="directory && !browsing" class="workspace-directory-empty">
                当前目录没有可浏览的文件夹
              </div>
            </a-spin>
          </div>
        </template>

        <footer class="workspace-picker-footer">
          <div class="workspace-current-selection">
            <span>当前工作空间</span>
            <strong :title="directory?.currentPath">{{ directory?.currentPath || "正在读取..." }}</strong>
          </div>
          <div class="workspace-picker-actions">
            <a-button
              type="primary"
              :loading="launchPreviewLoading"
              :disabled="browsing || launchLocked || launchPreviewLoading || !directory || apiKeyLoading || usableApiKeys.length === 0 || cliOptions.length === 0 || terminalOptions.length === 0 || historySelectionMissing"
              @click="emit('launch', directory?.currentPath)"
            >
              <template #icon><icon-launch /></template>
              {{ sessionMode === "history" ? "继续" : "启动" }} {{ cliLabel }}
            </a-button>
          </div>
        </footer>
      </section>
    </div>
  </a-modal>
</template>
