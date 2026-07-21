<script setup lang="ts">
import { computed, ref } from "vue";
import {
  IconBookmark,
  IconDelete,
  IconFolder,
  IconHome,
  IconLaunch,
  IconLoading,
  IconRefresh,
  IconRight,
  IconUp,
} from "@arco-design/web-vue/es/icon";
import type {
  LivenessCliKind,
  Provider,
  ProviderApiKeyOption,
  Workspace,
  WorkspaceDirectoryListing,
} from "../stores/providers";
import BrandIcon, { type BrandIconName } from "./BrandIcon.vue";
import ProviderAuthIcon from "./ProviderAuthIcon.vue";

const props = defineProps<{
  visible: boolean;
  provider: Provider | null;
  cliKind: LivenessCliKind;
  apiKeys: ProviderApiKeyOption[];
  apiKeyLoading: boolean;
  apiKeyError: string;
  apiKeyTokenId: string;
  selectedModel: string;
  workspaces: Workspace[];
  directory: WorkspaceDirectoryListing | null;
  pathDraft: string;
  browsing: boolean;
  launchingPath: string | null;
  forgettingPath: string | null;
  error: string;
}>();

const emit = defineEmits<{
  "update:visible": [visible: boolean];
  "update:pathDraft": [path: string];
  "update:cliKind": [kind: LivenessCliKind];
  "update:apiKeyTokenId": [tokenId: string];
  "update:selectedModel": [model: string];
  browse: [path?: string];
  launch: [path?: string];
  forget: [path: string];
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
const cliLabel = computed(() => (props.cliKind === "codex" ? "Codex" : "Claude Code"));
const preferredModel = computed(() => props.provider?.cli.preferredModel?.trim() || "");

function cliBrand(kind: LivenessCliKind): BrandIconName {
  return kind === "codex" ? "codex" : "claude";
}
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
      tokenId: "",
      status: "enabled",
      usedQuota: 0,
      remainQuota: 0,
      unlimitedQuota: false,
      group: "",
      modelLimitsEnabled: false,
      modelLimits: [],
      allowIps: [],
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
</script>

<template>
  <a-modal
    :visible="visible"
    width="min(940px, calc(100vw - 32px))"
    modal-class="workspace-picker-modal"
    title-align="start"
    :footer="false"
    :closable="!launching"
    :mask-closable="!launching"
    :esc-to-close="!launching"
    unmount-on-close
    @update:visible="emit('update:visible', $event)"
  >
    <template #title>{{ modalTitle }}</template>

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
              :title="`在 ${workspace.path} 启动 ${cliLabel}`"
              :disabled="launching"
              @click="emit('launch', workspace.path)"
            >
              <span class="workspace-history-icon" aria-hidden="true">
                <icon-bookmark />
              </span>
              <span class="workspace-history-copy">
                <strong>{{ workspaceName(workspace.path) }}</strong>
                <span :title="workspace.path">{{ workspace.path }}</span>
              </span>
              <icon-loading
                v-if="launchingPath === workspace.path"
                class="workspace-history-launch-icon"
              />
              <icon-launch v-else class="workspace-history-launch-icon" />
            </button>
            <div class="workspace-history-actions">
              <a-tooltip content="浏览此工作空间">
                <a-button
                  shape="circle"
                  size="mini"
                  :disabled="browsing || launching"
                  aria-label="浏览此工作空间"
                  @click="emit('browse', workspace.path)"
                >
                  <template #icon><icon-folder /></template>
                </a-button>
              </a-tooltip>
              <a-tooltip content="移除工作空间记录">
                <a-button
                  shape="circle"
                  size="mini"
                  status="danger"
                  :loading="forgettingPath === workspace.path"
                  :disabled="launching"
                  aria-label="移除工作空间记录"
                  @click="emit('forget', workspace.path)"
                >
                  <template #icon><icon-delete /></template>
                </a-button>
              </a-tooltip>
            </div>
          </div>
        </div>
      </aside>

      <section class="workspace-browser">
        <section class="workspace-launch-config" aria-label="临时 CLI 启动配置">
          <div class="workspace-launch-config-kind">
            <span class="workspace-config-label">CLI</span>
            <a-radio-group v-model="cliKindModel" type="button" size="small">
              <a-radio value="codex" class="workspace-cli-radio" title="Codex" aria-label="Codex">
                <BrandIcon :brand="cliBrand('codex')" :size="18" />
              </a-radio>
              <a-radio
                value="claudeCode"
                class="workspace-cli-radio"
                title="Claude Code"
                aria-label="Claude Code"
              >
                <BrandIcon :brand="cliBrand('claudeCode')" :size="18" />
              </a-radio>
            </a-radio-group>
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
              :disabled="launching"
              placeholder="选择 API Key"
            >
              <a-option v-for="option in effectiveApiKeys" :key="option.tokenId" :value="option.tokenId">
                {{ option.name || "未命名 API Key" }}
              </a-option>
            </a-select>
          </div>
          <div class="workspace-launch-config-field workspace-launch-model-field">
            <span class="workspace-config-label">模型</span>
            <div v-if="preferredModel" class="workspace-fixed-credential workspace-fixed-model">
              <span :title="preferredModel">{{ preferredModel }}</span>
            </div>
            <a-select
              v-else-if="modelOptions.length > 0"
              v-model="selectedModelModel"
              size="small"
              allow-search
              allow-clear
              :disabled="launching"
              placeholder="选择模型"
            >
              <a-option v-for="model in modelOptions" :key="model" :value="model">
                {{ model }}
              </a-option>
            </a-select>
            <a-input
              v-else
              v-model="selectedModelModel"
              size="small"
              :disabled="launching"
              placeholder="输入模型名称（可选）"
            />
          </div>
        </section>
        <a-alert v-if="apiKeyError" type="warning" :content="`API Key 列表读取失败：${apiKeyError}`" />
        <div class="workspace-browser-toolbar">
          <div class="workspace-browser-navigation">
            <a-tooltip content="主目录">
              <a-button
                shape="circle"
                :disabled="browsing || launching || !directory"
                aria-label="主目录"
                @click="emit('browse', directory?.homePath)"
              >
                <template #icon><icon-home /></template>
              </a-button>
            </a-tooltip>
            <a-tooltip content="上级目录">
              <a-button
                shape="circle"
                :disabled="browsing || launching || !directory?.parentPath"
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
                :disabled="launching || !directory"
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
            :disabled="launching"
            placeholder="输入工作空间路径"
            @keyup.enter="browseDraftPath"
          >
            <template #prefix><icon-folder /></template>
          </a-input>
          <a-tooltip content="打开路径">
            <a-button
              type="primary"
              shape="circle"
              :disabled="browsing || launching || !pathModel.trim()"
              aria-label="打开路径"
              @click="browseDraftPath"
            >
              <template #icon><icon-right /></template>
            </a-button>
          </a-tooltip>
        </div>

        <a-alert v-if="error" type="error" :content="error" show-icon />

        <div class="workspace-directory-scroll">
          <a-spin class="workspace-directory-spin" :loading="browsing">
            <div v-if="directory && visibleDirectories.length > 0" class="workspace-directory-list">
              <button
                v-for="entry in visibleDirectories"
                :key="entry.path"
                type="button"
                class="workspace-directory-item"
                :disabled="browsing || launching"
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

        <footer class="workspace-picker-footer">
          <div class="workspace-current-selection">
            <span>当前工作空间</span>
            <strong :title="directory?.currentPath">{{ directory?.currentPath || "正在读取..." }}</strong>
          </div>
          <div class="workspace-picker-actions">
            <a-button :disabled="launching" @click="emit('update:visible', false)">取消</a-button>
            <a-button
              type="primary"
              :loading="launchingPath === directory?.currentPath"
              :disabled="browsing || launching || !directory || apiKeyLoading || effectiveApiKeys.length === 0"
              @click="emit('launch', directory?.currentPath)"
            >
              <template #icon><icon-launch /></template>
              启动 {{ cliLabel }}
            </a-button>
          </div>
        </footer>
      </section>
    </div>
  </a-modal>
</template>
