<script setup lang="ts">
import { computed, ref } from "vue";
import { Message } from "@arco-design/web-vue";
import {
  IconCopy,
  IconDelete,
  IconFolder,
  IconHome,
  IconLaunch,
  IconRefresh,
  IconRight,
  IconUp,
} from "@arco-design/web-vue/es/icon";
import type {
  LivenessCliKind,
  CliSessionSummary,
  Provider,
  ProviderApiKeyOption,
  TemporaryCliSessionMode,
  TemporaryCliTerminalKind,
  Workspace,
  WorkspaceDirectoryListing,
} from "../stores/providers";
import type { SelectOption } from "../utils/liveness-options";
import { copyText } from "../composables/useClipboard";
import CliIconSelector from "./CliIconSelector.vue";
import ProviderAuthIcon from "./ProviderAuthIcon.vue";
import TerminalIconSelector from "./TerminalIconSelector.vue";

const props = defineProps<{
  visible: boolean;
  provider: Provider | null;
  cliKind: LivenessCliKind;
  cliOptions: SelectOption<LivenessCliKind>[];
  apiKeys: ProviderApiKeyOption[];
  apiKeyLoading: boolean;
  apiKeyError: string;
  apiKeyTokenId: string;
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
  launchingPath: string | null;
  launchProgress: number;
  launchStage: string;
  launchPreviewVisible: boolean;
  launchPreviewLoading: boolean;
  forgettingPath: string | null;
  error: string;
  historySessions: CliSessionSummary[];
  historyLoading: boolean;
  historyError: string;
}>();

const emit = defineEmits<{
  "update:visible": [visible: boolean];
  "update:pathDraft": [path: string];
  "update:cliKind": [kind: LivenessCliKind];
  "update:apiKeyTokenId": [tokenId: string];
  "update:selectedModel": [model: string];
  "update:sessionName": [name: string];
  "update:sessionMode": [mode: TemporaryCliSessionMode];
  "update:selectedResumeId": [id: string];
  "update:terminalKind": [kind: TemporaryCliTerminalKind];
  browse: [path?: string];
  launch: [path?: string];
  forget: [path: string];
  "select-session": [session: CliSessionSummary];
  "refresh-sessions": [path?: string];
}>();

const showHidden = ref(false);

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

const launching = computed(() => Boolean(props.launchingPath));
const launchLocked = computed(
  () => launching.value || props.launchPreviewVisible || props.launchPreviewLoading,
);
const cliLabel = computed(() => (props.cliKind === "codex" ? "Codex" : "Claude Code"));
const codexNamingHint = computed(
  () => props.cliKind === "codex" && props.sessionMode === "new" && !props.canNameSession,
);
const preferredModel = computed(() => props.provider?.cli.preferredModel?.trim() || "");
const fixedModel = computed(() => (props.sessionMode === "new" ? preferredModel.value : ""));
const modelPlaceholder = computed(() =>
  props.sessionMode === "new" ? "选择或输入模型（可选）" : "不指定则沿用原会话模型",
);
const launchProgressPercent = computed(() =>
  Math.min(1, Math.max(0, props.launchProgress / 100)),
);
const selectedResumeIdModel = computed(() => props.selectedResumeId);
const selectedSession = computed(() =>
  props.historySessions.find((session) => session.id === props.selectedResumeId) ?? null,
);
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
  const keys: ProviderApiKeyOption[] = [];
  if (providerKey) {
    keys.push({
      name: "当前配置 API Key",
      key: providerKey,
      maskedKey: "",
      keyAvailable: true,
      tokenId: "",
      userId: "",
      status: "enabled",
      usedQuota: 0,
      remainQuota: 0,
      usedQuotaRaw: 0,
      remainQuotaRaw: 0,
      unlimitedQuota: false,
      group: "",
      crossGroupRetry: false,
      modelLimitsEnabled: false,
      modelLimits: [],
      allowIps: [],
      quotaDisplayType: "currency",
      currencySymbol: "$",
    });
  }
  const knownKeys = new Set([providerKey]);
  for (const option of props.apiKeys) {
    const key = option.key.trim();
    if (!key || knownKeys.has(key)) {
      continue;
    }
    knownKeys.add(key);
    keys.push(option);
  }
  return keys;
});
const hasSingleApiKey = computed(() => effectiveApiKeys.value.length === 1);
const singleApiKey = computed(() => effectiveApiKeys.value[0] ?? null);
const cliKindModel = computed({
  get: () => props.cliKind,
  set: (value: LivenessCliKind) => emit("update:cliKind", value),
});
const apiKeyModel = computed({
  get: () => props.apiKeyTokenId,
  set: (value: string) => emit("update:apiKeyTokenId", value),
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
  if (!visible && launchLocked.value) return;
  emit("update:visible", visible);
}

function sessionModelLabel(session: CliSessionSummary) {
  if (session.models.length > 1) {
    return `多模型（最近：${session.model || session.models[session.models.length - 1]}）`;
  }
  return session.model || "未记录模型";
}

function sessionTime(value: string | null) {
  if (!value) return "时间未知";
  const date = new Date(value);
  return Number.isNaN(date.getTime()) ? value : date.toLocaleString();
}

async function copySessionId(id: string) {
  const value = id.trim();
  if (!value) return;
  try {
    await copyText(value);
    Message.success("已复制 Resume ID");
  } catch (error) {
    Message.error(error instanceof Error ? error.message : String(error));
  }
}
</script>

<template>
  <a-modal
    :visible="visible"
    width="min(940px, calc(100vw - 32px))"
    modal-class="surface-modal workspace-picker-modal"
    title-align="start"
    :footer="false"
    :closable="!launchLocked"
    :mask-closable="!launchLocked"
    :esc-to-close="!launchLocked"
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
              <span :title="singleApiKey.name || '当前配置 API Key'">
                {{ singleApiKey.name || "当前配置 API Key" }}
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
              <a-option v-for="option in effectiveApiKeys" :key="option.tokenId" :value="option.tokenId">
                {{ option.name || "未命名 API Key" }}
              </a-option>
            </a-select>
          </div>
          <div class="workspace-launch-config-field workspace-launch-model-field">
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
              <a-radio value="history">继续历史会话</a-radio>
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
            v-else-if="codexNamingHint"
            class="workspace-session-capability-note"
            type="info"
            show-icon
          >
            Codex 当前不支持启动前命名；启动后可在终端输入 /new 名称 或 /rename
          </a-alert>
          <div v-if="sessionMode !== 'new'" class="workspace-session-history">
            <div class="workspace-session-history-toolbar">
              <div>
                <strong>历史会话</strong>
                <span>选择会话后点击底部继续按钮启动 CLI</span>
              </div>
              <a-tooltip content="刷新历史会话">
                <a-button
                  shape="circle"
                  size="mini"
                  :loading="historyLoading"
                  :disabled="launchLocked || !directory"
                  aria-label="刷新历史会话"
                  @click="emit('refresh-sessions', directory?.currentPath)"
                >
                  <template #icon><icon-refresh /></template>
                </a-button>
              </a-tooltip>
            </div>
            <a-alert v-if="historyError" type="warning" show-icon>
              <template #title>历史索引暂不可用</template>
              <template #default>{{ historyError }}。请检查 CLI 状态目录后重试。</template>
            </a-alert>
            <a-spin :loading="historyLoading" class="workspace-session-history-spin">
              <div v-if="!historyLoading && historySessions.length === 0" class="workspace-session-empty">
                <strong>当前工作空间没有可展示的历史会话</strong>
                <span>请先在该工作空间创建一条有效会话。</span>
              </div>
              <div v-else class="workspace-session-list">
                <div
                  v-for="session in historySessions"
                  :key="session.id"
                  class="workspace-session-item"
                  :class="{ selected: session.id === selectedResumeIdModel, disabled: !session.canResume }"
                >
                  <button
                    type="button"
                    class="workspace-session-select"
                    :disabled="launchLocked || !session.canResume"
                    :aria-pressed="session.id === selectedResumeIdModel"
                    :title="session.canResume ? `选择会话：${session.title}` : '该会话已归档，无法继续'"
                    @click="emit('select-session', session)"
                  >
                    <span class="workspace-session-item-main">
                      <strong>{{ session.title }}</strong>
                      <span v-if="session.preview" class="workspace-session-preview">{{ session.preview }}</span>
                      <span class="workspace-session-meta">
                        <span>模型：{{ sessionModelLabel(session) }}</span>
                        <span>更新时间：{{ sessionTime(session.updatedAt) }}</span>
                      </span>
                    </span>
                    <span class="workspace-session-item-side">
                      <span class="workspace-session-id" :title="`Resume ID：${session.id}`">{{ session.id }}</span>
                      <span v-if="session.archived" class="workspace-session-archived">已归档</span>
                    </span>
                  </button>
                  <a-tooltip content="复制 Resume ID">
                    <a-button
                      class="workspace-session-copy"
                      shape="circle"
                      size="mini"
                      :disabled="launchLocked || !session.id"
                      aria-label="复制 Resume ID"
                      @click.stop="copySessionId(session.id)"
                    >
                      <template #icon><icon-copy /></template>
                    </a-button>
                  </a-tooltip>
                </div>
              </div>
            </a-spin>
            <a-alert
              v-if="sessionMode === 'history' && selectedSession"
              class="workspace-session-selected-note"
              type="success"
              show-icon
            >
              已选择：{{ selectedSession.title }}。不选择模型时将沿用历史会话模型。
            </a-alert>
          </div>
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

        <div v-if="launching" class="workspace-launch-progress" aria-live="polite">
          <div class="workspace-launch-progress-label">
            <strong>{{ launchStage || `正在启动 ${cliLabel}` }}</strong>
            <span>{{ Math.round(launchProgress) }}%</span>
          </div>
          <a-progress
            :percent="launchProgressPercent"
            :show-text="false"
            size="small"
            animation
          />
        </div>

        <footer class="workspace-picker-footer">
          <div class="workspace-current-selection">
            <span>当前工作空间</span>
            <strong :title="directory?.currentPath">{{ directory?.currentPath || "正在读取..." }}</strong>
          </div>
          <div class="workspace-picker-actions">
            <a-button
              type="primary"
              :loading="launchingPath === directory?.currentPath"
              :disabled="browsing || launchLocked || !directory || apiKeyLoading || effectiveApiKeys.length === 0 || cliOptions.length === 0 || terminalOptions.length === 0 || historySelectionMissing"
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
