<script setup lang="ts">
import SelectedModelMetadata from "./SelectedModelMetadata.vue";
import AgentModelSelect from "./AgentModelSelect.vue";
import TemporaryCliSettingsPanel from "./TemporaryCliSettingsPanel.vue";
import TemporaryCliDirectoryPanel from "./TemporaryCliDirectoryPanel.vue";
import type {
  TemporaryCliRunSettings,
  TemporaryCliSettingsCapabilities,
} from "../stores/temporary-cli-settings-types";
import type { AgentSettingValueSummary } from "../stores/agent-settings-types";
import {
  knownSettingValue,
  settingValueBadge,
  settingValueDescription,
  settingValueLabel,
} from "../utils/agent-setting-display";
import { computed, shallowRef } from "vue";
import { Message } from "@arco-design/web-vue";
import {
  IconCode,
  IconFolder,
  IconInfoCircle,
  IconLaunch,
  IconRefresh,
  IconUser,
} from "@arco-design/web-vue/es/icon";
import {
  type AgentCliKind,
  type CliSessionIndexState,
  type Provider,
  type ProviderApiKeyOption,
  type TemporaryCliSessionMode,
  type TemporaryCliTerminalKind,
  type Workspace,
  type WorkspaceDirectoryListing,
} from "../stores/providers";
import type {
  AgentSessionRoleFilter,
  AgentSessionRow,
} from "../stores/agent-session-types";
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
import {
  providerApiKeyDisplayName,
  providerApiKeySecondaryName,
  providerDisplayLabel,
} from "../utils/provider-display";

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
  historyResults: AgentSessionRow[];
  historyRoleFilter: AgentSessionRoleFilter;
  historyTotal: number | null;
  historyHasMore: boolean;
  historyLoadingMore: boolean;
  selectedSessionRef: string;
  historyLoading: boolean;
  historyError: string;
  historyIndexState: CliSessionIndexState;
  historyIndexMessage: string;
  selectedSessionTitle: string;
  selectedSessionWorkdir: string;
}>();

const emit = defineEmits<{
  "update:visible": [visible: boolean];
  "update:pathDraft": [path: string];
  "update:cliKind": [kind: AgentCliKind];
  "update:apiKeyLocalId": [localId: string];
  "update:selectedModel": [model: string];
  "update:sessionName": [name: string];
  "update:sessionMode": [mode: TemporaryCliSessionMode];
  "update:terminalKind": [kind: TemporaryCliTerminalKind];
  "update:historyQuery": [query: string];
  "update:historyRoleFilter": [role: AgentSessionRoleFilter];
  browse: [path?: string];
  launch: [path?: string];
  forget: [path: string];
  "select-session": [row: AgentSessionRow];
  "view-session": [row: AgentSessionRow];
  "view-parent-session": [sessionRef: string];
  "load-more-sessions": [];
  "refresh-sessions": [];
}>();

const runSettings = defineModel<TemporaryCliRunSettings>("runSettings", {
  required: true,
});
const capabilities = shallowRef<TemporaryCliSettingsCapabilities | null>(null);
const store = useCliRuntimeStore();

async function refreshCliTools() {
  try {
    await store.probeCliTools(true);
  } catch (error) {
    Message.error(String(error));
  }
}

const pathModel = computed({
  get: () => props.pathDraft,
  set: (value: string) => emit("update:pathDraft", value),
});

const providerLabel = computed(() =>
  props.provider ? providerDisplayLabel(props.provider) : "",
);

const launchLocked = computed(
  () => props.launchPreviewVisible || props.launchPreviewLoading,
);
const cliLabel = computed(() =>
  agentCliLabel(store.cliEnvironmentProbe, props.cliKind),
);
const selectedCliTool = computed(() =>
  agentCliTool(store.cliEnvironmentProbe, props.cliKind),
);
const supportsModelSelection = computed(() =>
  Boolean(selectedCliTool.value?.capabilities.modelSelection),
);
const supportsSessionHistory = computed(
  () =>
    Boolean(selectedCliTool.value?.capabilities.sessionHistory) &&
    Boolean(selectedCliTool.value?.capabilities.sessionSearch) &&
    Boolean(selectedCliTool.value?.capabilities.sessionDetail) &&
    Boolean(selectedCliTool.value?.capabilities.sessionResume),
);
const sessionNamingHint = computed(() => {
  if (props.sessionMode !== "new" || props.canNameSession) return "";
  return (
    selectedCliTool.value?.sessionNameHint ||
    `${cliLabel.value} 当前不支持启动前命名`
  );
});
const historyModel = computed(() =>
  props.sessionMode === "history"
    ? (props.historyResults.find(
        (row) => row.sessionRef === props.selectedSessionRef,
      )?.session.model ?? "")
    : "",
);
const inheritedModelValue = computed<AgentSettingValueSummary>(() => {
  if (props.sessionMode === "history")
    return {
      value: historyModel.value || null,
      label: historyModel.value
        ? `原会话模型：${historyModel.value}`
        : "恢复原会话模型",
      source: "所选历史会话记录",
      state: historyModel.value ? "configured" : "default",
      detail: historyModel.value
        ? "未指定本次模型时，继续使用原会话的模型。"
        : "当前会话信息没有提供具体模型 ID，继续会话时由 Agent 从原记录恢复。",
    };
  if (capabilities.value?.modelCatalog.cliKind === props.cliKind)
    return capabilities.value.defaultModel;
  return {
    value: null,
    label: "模型配置尚未读取",
    source: "",
    state: "unset",
    detail: "读取完成后显示实际模型与配置来源，也可直接选择模型。",
  };
});
const modelValue = computed<AgentSettingValueSummary>(() =>
  props.selectedModel.trim()
    ? {
        value: props.selectedModel.trim(),
        label: props.selectedModel.trim(),
        source: "本次启动选择",
        state: "configured",
        detail: "",
      }
    : inheritedModelValue.value,
);
const effectiveModel = computed(
  () => knownSettingValue(modelValue.value) ?? "",
);
const historyQueryModel = computed({
  get: () => props.historyQuery,
  set: (value: string) => emit("update:historyQuery", value),
});
const historySelectionMissing = computed(
  () =>
    props.sessionMode === "history" &&
    (!props.selectedResumeId || !props.selectedSessionWorkdir),
);
const launchWorkdir = computed(() =>
  props.sessionMode === "history"
    ? props.selectedSessionWorkdir
    : props.directory?.currentPath || "",
);

const effectiveApiKeys = computed(() => {
  const providerKey = props.provider?.auth.apiKey.trim() || "";
  return effectiveProviderApiKeyOptions(providerKey, props.apiKeys);
});
const usableApiKeys = computed(() =>
  effectiveApiKeys.value.filter(isProviderApiKeyUsable),
);
const hasSingleApiKey = computed(() => usableApiKeys.value.length === 1);
const singleApiKey = computed(() => usableApiKeys.value[0] ?? null);
// An OAuth card launches with its own vendor credential, so this picker never
// asks for an API key or blocks the launch when none exists.
const oauthAccount = computed(() =>
  props.provider?.identity.protocol === "oauth" ? props.provider.auth.oauth : null,
);
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
  set: (value: string | undefined) => emit("update:selectedModel", value || ""),
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
function handleVisibleChange(visible: boolean) {
  emit("update:visible", visible);
}
</script>

<template>
  <a-modal
    :visible="visible"
    width="min(1220px, calc(100vw - 48px))"
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
      <div class="surface-modal-title cli-launcher-title">
        <span class="surface-modal-title-icon"><icon-code /></span>
        <span class="surface-modal-title-copy"
          ><strong>启动临时 CLI</strong></span
        >
        <span
          v-if="providerLabel"
          class="cli-launcher-provider"
          :title="providerLabel"
          >{{ providerLabel }}</span
        >
      </div>
    </template>

    <div
      class="cli-launcher"
      :class="{ 'is-history': sessionMode === 'history' }"
    >
      <div class="cli-launcher-panes">
        <aside class="cli-launcher-navigation">
          <header class="cli-launcher-session-mode">
            <nav aria-label="会话启动方式">
              <button
                type="button"
                :aria-pressed="sessionMode === 'new'"
                :disabled="launchLocked"
                @click="sessionModeModel = 'new'"
              >
                新会话
              </button>
              <button
                v-if="supportsSessionHistory"
                type="button"
                :aria-pressed="sessionMode === 'history'"
                :disabled="launchLocked"
                @click="sessionModeModel = 'history'"
              >
                继续会话
              </button>
            </nav>
          </header>
          <TemporaryCliDirectoryPanel
            v-if="sessionMode === 'new'"
            v-model:path-draft="pathModel"
            :active="visible"
            :workspaces="workspaces"
            :directory="directory"
            :browsing="browsing"
            :forgetting-path="forgettingPath"
            :disabled="launchLocked"
            @browse="emit('browse', $event)"
            @forget="emit('forget', $event)"
          />
          <WorkspaceSessionHistoryPanel
            v-else
            v-model:query="historyQueryModel"
            :results="historyResults"
            :role-filter="historyRoleFilter"
            :total="historyTotal"
            :has-more="historyHasMore"
            :loading-more="historyLoadingMore"
            :selected-session-ref="selectedSessionRef"
            :loading="historyLoading"
            :error="historyError"
            :index-state="historyIndexState"
            :index-message="historyIndexMessage"
            :selected-session-title="selectedSessionTitle"
            :disabled="launchLocked"
            @select-session="emit('select-session', $event)"
            @view-session="emit('view-session', $event)"
            @view-parent="emit('view-parent-session', $event)"
            @load-more="emit('load-more-sessions')"
            @update:role-filter="emit('update:historyRoleFilter', $event)"
            @refresh="emit('refresh-sessions')"
          />
        </aside>

        <section
          class="cli-launcher-configuration"
          aria-label="运行环境与会话配置"
        >
          <section class="cli-launcher-runtime" aria-label="运行环境">
            <div class="cli-runtime-agent-row">
              <span class="cli-runtime-label">Agent</span>
              <CliIconSelector
                v-model="cliKindModel"
                :options="cliOptions"
                show-labels
                :disabled="launchLocked"
              />
              <a-tooltip content="重新检测本机 Agent"
                ><a-button
                  type="text"
                  size="small"
                  :loading="store.cliEnvironmentLoading"
                  :disabled="launchLocked"
                  aria-label="重新检测本机 Agent"
                  @click="refreshCliTools"
                  ><template #icon><icon-refresh /></template></a-button
              ></a-tooltip>
            </div>
            <div class="cli-runtime-terminal-row">
              <span class="cli-runtime-label">终端</span>
              <TerminalIconSelector
                v-model="terminalKindModel"
                :options="terminalOptions"
                show-labels
                :disabled="launchLocked"
              />
            </div>
            <div class="cli-runtime-fields">
              <div class="cli-runtime-model">
                <div class="cli-runtime-field-heading">
                  <label for="cli-launcher-model">模型</label>
                  <a-popover
                    v-if="capabilities"
                    trigger="click"
                    position="bottom"
                    title="本次使用的模型"
                    content-class="cli-native-explanation"
                  >
                    <button
                      type="button"
                      class="cli-runtime-source-button"
                      aria-label="查看模型配置来源"
                      title="查看模型信息与配置来源"
                    >
                      <icon-info-circle />
                    </button>
                    <template #content
                      ><dl>
                        <dt>配置键与启动参数</dt>
                        <dd>
                          <code>{{ capabilities.modelNativeKey }}</code>
                        </dd>
                        <dt>模型</dt>
                        <dd>{{ settingValueLabel(modelValue) }}</dd>
                        <dt>解析状态</dt>
                        <dd>{{ settingValueBadge(modelValue) }}</dd>
                        <template v-if="modelValue.source">
                          <dt>来源</dt>
                          <dd>{{ modelValue.source }}</dd>
                        </template>
                        <template v-if="modelValue.detail">
                          <dt>说明</dt>
                          <dd>{{ modelValue.detail }}</dd>
                        </template>
                      </dl>
                      <SelectedModelMetadata
                        :model="effectiveModel"
                        :active="visible"
                      />
                    </template>
                  </a-popover>
                </div>
                <AgentModelSelect
                  v-if="supportsModelSelection"
                  id="cli-launcher-model"
                  v-model:value="selectedModelModel"
                  :native-choices="capabilities?.modelChoices ?? []"
                  :catalog="capabilities?.modelCatalog"
                  :provider-models="provider?.capabilities.availableModels"
                  :disabled="launchLocked"
                  :default-label="settingValueLabel(inheritedModelValue)"
                  :default-description="
                    settingValueDescription(inheritedModelValue)
                  "
                  :default-badge="settingValueBadge(inheritedModelValue)"
                  aria-label="选择或输入本次模型"
                />
                <div
                  v-else
                  class="cli-runtime-readonly"
                  :title="effectiveModel"
                >
                  {{ settingValueLabel(modelValue) }}
                </div>
              </div>
              <div class="cli-runtime-key">
                <div class="cli-runtime-field-heading">
                  <label for="cli-launcher-key">{{
                    oauthAccount ? "账号授权" : "API Key"
                  }}</label>
                </div>
                <div
                  v-if="oauthAccount"
                  class="cli-runtime-readonly cli-runtime-single-key"
                >
                  <IconUser /><span
                    :title="oauthAccount.email || provider?.displayLabel"
                    >{{
                      oauthAccount.email ||
                      provider?.displayLabel ||
                      oauthAccount.vendor
                    }}</span
                  >
                </div>
                <div
                  v-else-if="hasSingleApiKey && singleApiKey"
                  class="cli-runtime-readonly cli-runtime-single-key"
                >
                  <ProviderAuthIcon mode="apiKey" /><span
                    :title="providerApiKeyDisplayName(singleApiKey)"
                    >{{ providerApiKeyDisplayName(singleApiKey) }}</span
                  >
                </div>
                <a-select
                  v-else
                  id="cli-launcher-key"
                  v-model="apiKeyModel"
                  :loading="apiKeyLoading"
                  :disabled="launchLocked"
                  placeholder="选择 API Key"
                  aria-label="本次使用的 API Key"
                >
                  <a-option
                    v-for="option in effectiveApiKeys"
                    :key="option.localId || option.tokenId || option.key"
                    :value="option.localId || option.tokenId || option.key"
                    :disabled="!isProviderApiKeyUsable(option)"
                  >
                    <span>{{ providerApiKeyDisplayName(option) }}</span
                    ><small
                      v-if="providerApiKeySecondaryName(option)"
                      class="cli-runtime-key-secondary"
                    >
                      · {{ providerApiKeySecondaryName(option) }}</small
                    ><span v-if="!isProviderApiKeyUsable(option)"
                      >（不可用）</span
                    >
                  </a-option>
                </a-select>
              </div>
            </div>
            <p v-if="apiKeyError" class="cli-run-error" role="alert">
              API Key 列表读取失败：{{ apiKeyError }}
            </p>
          </section>
          <TemporaryCliSettingsPanel
            v-model="runSettings"
            v-model:session-name="sessionNameModel"
            :active="visible"
            :cli-kind="cliKind"
            :cli-path="selectedCliTool?.path || ''"
            :workdir="launchWorkdir"
            :selected-model="selectedModel"
            :inherited-model="historyModel"
            :provider-models="provider?.capabilities.availableModels"
            :session-mode="sessionMode"
            :can-name-session="canNameSession"
            :session-naming-hint="sessionNamingHint"
            :disabled="launchLocked"
            @capabilities="capabilities = $event"
          />
        </section>
      </div>

      <footer class="cli-launcher-footer">
        <div class="cli-launcher-selection">
          <icon-folder />
          <div class="cli-launcher-selection-copy">
            <span v-if="sessionMode === 'history'">{{
              selectedSessionTitle || "选择一个历史会话"
            }}</span>
            <strong :title="launchWorkdir">{{
              launchWorkdir ||
              (sessionMode === "history"
                ? "使用所选会话的原目录"
                : "正在读取目录…")
            }}</strong>
            <p v-if="error" class="cli-run-error" role="alert">{{ error }}</p>
          </div>
        </div>
        <div class="cli-launcher-actions">
          <span class="cli-launcher-retention">{{
            sessionMode === "history"
              ? "继续原记录"
              : runSettings.keepSession
                ? "保存会话"
                : "退出后丢弃"
          }}</span>
          <a-button
            type="primary"
            size="large"
            :loading="launchPreviewLoading"
            :disabled="
              (sessionMode === 'new' && browsing) ||
              launchLocked ||
              !launchWorkdir ||
              apiKeyLoading ||
              (!oauthAccount && usableApiKeys.length === 0) ||
              cliOptions.length === 0 ||
              terminalOptions.length === 0 ||
              historySelectionMissing
            "
            @click="emit('launch', launchWorkdir)"
          >
            <template #icon><icon-launch /></template
            >{{ sessionMode === "history" ? "继续" : "启动" }} {{ cliLabel }}
          </a-button>
        </div>
      </footer>
    </div>
  </a-modal>
</template>
