<script setup lang="ts">
import { computed, onBeforeUnmount, reactive, watch } from "vue";
import WorkspaceCardActionGroup from "../workspace-card/WorkspaceCardActionGroup.vue";
import ProviderCardMenuPopover from "./ProviderCardMenuPopover.vue";
import ProviderAppImportList from "./ProviderAppImportList.vue";
import {
  IconApps,
  IconBarChart,
  IconCalendar,
  IconCopy,
  IconFile,
  IconLink,
  IconLoading,
  IconLock,
  IconSettings,
  IconSync,
  IconThunderbolt,
} from "@arco-design/web-vue/es/icon";
import { PackagePlus } from "@lucide/vue";
import ProviderAuthIcon from "../ProviderAuthIcon.vue";
import type { Provider } from "../../stores/providers";
import {
  supportsAccountManagement,
  supportsCheckIn,
  supportsInvitation,
} from "../../utils/provider-actions";
import { providerCardScopeNoun } from "../../utils/provider-display";
import type { ProviderAppImportTarget } from "../../stores/provider-types";

/// 四类卡片的二级菜单是同一套：复制 / 数据 / 站点 / 导出。菜单名按卡片自己的
/// 称呼（中转站、账号、网关）读，但分组的数量、顺序和图标不随卡型改变。
const props = withDefaults(
  defineProps<{
    provider: Provider;
    probingCapabilities?: boolean;
  }>(),
  {
    probingCapabilities: false,
  },
);

const emit = defineEmits<{
  probeCapabilities: [provider: Provider];
  openApiKeyManager: [provider: Provider];
  openAvailableModels: [provider: Provider];
  openUsage: [provider: Provider];
  openRequestLogs: [provider: Provider];
  openPasswordChange: [provider: Provider];
  openLivenessDetails: [provider: Provider];
  openCheckInRecords: [provider: Provider];
  addProviderToApp: [provider: Provider, target: ProviderAppImportTarget];
  copyUrl: [provider: Provider];
  copyInvite: [provider: Provider];
  copySecret: [
    provider: Provider,
    field: "apiKey" | "accessToken" | "sessionCookie",
  ];
  interaction: [active: boolean];
}>();

/// 每个「…」各自记录展开状态：同时只开一个菜单，但关闭一个不影响另一个
/// 触发器的视觉状态。
const menuInteractions = reactive({
  copy: false,
  data: false,
  site: false,
  apps: false,
});
const noun = computed(() => providerCardScopeNoun(props.provider));
const groupLabel = computed(() => `${noun.value}功能`);
const copyMenuLabel = computed(() => `复制${noun.value}信息`);
const dataMenuLabel = computed(() => `查看${noun.value}数据`);
const siteMenuLabel = computed(() => `管理${noun.value}能力`);
const urlLabel = computed(() => `${noun.value} URL`);
const appImportOptions = computed(() => props.provider.actions.appImports);
const hasCopyActions = computed(() =>
  Boolean(
    props.provider.identity.baseUrl.trim() ||
    props.provider.auth.apiKey.trim() ||
    (props.provider.auth.mode !== "apiKey" &&
      (props.provider.auth.accessToken.trim() ||
        props.provider.auth.sessionCookie.trim())) ||
    (props.provider.runtime.enabled && supportsInvitation(props.provider)),
  ),
);
const canViewAvailableModels = computed(() =>
  Boolean(
    props.provider.actions.models.canSync ||
    (props.provider.capabilities.availableModels || []).length > 0,
  ),
);
const accountManagementAvailable = computed(() =>
  supportsAccountManagement(props.provider),
);
const canViewUsage = computed(
  () => props.provider.runtime.enabled && accountManagementAvailable.value,
);
const canViewRequestLogs = computed(
  () => props.provider.runtime.enabled && accountManagementAvailable.value,
);
const canViewLiveness = computed(
  () =>
    props.provider.liveness.enabled ||
    props.provider.liveness.records.length > 0,
);
const canViewCheckInRecords = computed(
  () =>
    props.provider.auth.mode !== "apiKey" &&
    (supportsCheckIn(props.provider) ||
      props.provider.automation.checkInRecords.length > 0),
);
const hasDataActions = computed(
  () =>
    canViewUsage.value ||
    canViewRequestLogs.value ||
    canViewLiveness.value ||
    canViewCheckInRecords.value,
);
const canProbeSite = computed(
  () => props.provider.runtime.enabled && accountManagementAvailable.value,
);
const canChangePassword = computed(() => accountManagementAvailable.value);
const hasManagedApiKeys = computed(() =>
  Boolean(
    props.provider.auth.apiKey.trim() ||
    props.provider.auth.apiKeyOptions.length > 0,
  ),
);
const showSiteApiKeyManagement = computed(
  () =>
    props.provider.auth.mode !== "apiKey" &&
    (props.provider.actions.apiKeyManagement || hasManagedApiKeys.value),
);
const hasSiteActions = computed(
  () =>
    canProbeSite.value ||
    showSiteApiKeyManagement.value ||
    canViewAvailableModels.value ||
    canChangePassword.value,
);
const hasSecondaryActions = computed(
  () =>
    hasCopyActions.value ||
    hasDataActions.value ||
    hasSiteActions.value ||
    appImportOptions.value.length > 0,
);
watch(
  () => Object.values(menuInteractions).some(Boolean),
  (active) => emit("interaction", active),
  { immediate: true },
);

onBeforeUnmount(() => emit("interaction", false));

function openDataAction(
  action: "usage" | "requestLogs" | "liveness" | "checkInRecords",
) {
  if (action === "usage") {
    emit("openUsage", props.provider);
  } else if (action === "requestLogs") {
    emit("openRequestLogs", props.provider);
  } else if (action === "liveness") {
    emit("openLivenessDetails", props.provider);
  } else {
    emit("openCheckInRecords", props.provider);
  }
}

function openSiteAction(action: "probe" | "keys" | "models" | "password") {
  if (action === "probe") {
    if (!props.provider.runtime.enabled || props.probingCapabilities) {
      return;
    }
    emit("probeCapabilities", props.provider);
  } else if (action === "keys") {
    emit("openApiKeyManager", props.provider);
  } else if (action === "models") {
    emit("openAvailableModels", props.provider);
  } else {
    emit("openPasswordChange", props.provider);
  }
}

function addProviderToApp(target: ProviderAppImportTarget) {
  emit("addProviderToApp", props.provider, target);
}

function copyProviderUrl() {
  emit("copyUrl", props.provider);
}

function copyProviderInvite() {
  emit("copyInvite", props.provider);
}

function copyProviderSecret(field: "apiKey" | "accessToken" | "sessionCookie") {
  emit("copySecret", props.provider, field);
}
</script>

<template>
  <WorkspaceCardActionGroup
    v-if="hasSecondaryActions"
    :label="groupLabel"
    divided
  >
    <ProviderCardMenuPopover
      v-if="hasCopyActions"
      tone="copy"
      :label="copyMenuLabel"
      @interaction="menuInteractions.copy = $event"
    >
      <template #icon><icon-copy /></template>
      <div class="provider-card-action-panel-title">复制</div>
      <div class="provider-card-action-list">
        <button
          role="menuitem"
          tabindex="-1"
          v-if="provider.identity.baseUrl.trim()"
          type="button"
          @click="copyProviderUrl"
        >
          <icon-link
            class="provider-card-action-icon provider-card-action-icon-url"
          />
          <span>{{ urlLabel }}</span>
        </button>
        <button
          role="menuitem"
          tabindex="-1"
          v-if="provider.auth.apiKey.trim()"
          type="button"
          @click="copyProviderSecret('apiKey')"
        >
          <ProviderAuthIcon mode="apiKey" />
          <span>API Key</span>
        </button>
        <button
          role="menuitem"
          tabindex="-1"
          v-if="
            provider.auth.mode !== 'apiKey' && provider.auth.accessToken.trim()
          "
          type="button"
          @click="copyProviderSecret('accessToken')"
        >
          <ProviderAuthIcon mode="accessToken" />
          <span>访问令牌</span>
        </button>
        <button
          role="menuitem"
          tabindex="-1"
          v-if="
            provider.auth.mode !== 'apiKey' &&
            provider.auth.sessionCookie.trim()
          "
          type="button"
          @click="copyProviderSecret('sessionCookie')"
        >
          <ProviderAuthIcon mode="session" />
          <span>Cookie</span>
        </button>
        <button
          role="menuitem"
          tabindex="-1"
          v-if="provider.runtime.enabled && supportsInvitation(provider)"
          type="button"
          @click="copyProviderInvite"
        >
          <icon-link
            class="provider-card-action-icon provider-card-action-icon-invite"
          />
          <span>邀请链接</span>
        </button>
      </div>
    </ProviderCardMenuPopover>

    <ProviderCardMenuPopover
      v-if="hasDataActions"
      tone="data"
      :label="dataMenuLabel"
      @interaction="menuInteractions.data = $event"
    >
      <template #icon><icon-bar-chart /></template>
      <div class="provider-card-action-panel-title">数据</div>
      <div class="provider-card-action-list">
        <button
          role="menuitem"
          tabindex="-1"
          v-if="canViewUsage"
          type="button"
          @click="openDataAction('usage')"
        >
          <icon-bar-chart
            class="provider-card-action-icon provider-card-action-icon-usage"
          />
          <span>用量趋势</span>
        </button>
        <button
          role="menuitem"
          tabindex="-1"
          v-if="canViewRequestLogs"
          type="button"
          @click="openDataAction('requestLogs')"
        >
          <icon-file
            class="provider-card-action-icon provider-card-action-icon-logs"
          />
          <span>请求日志</span>
        </button>
        <button
          role="menuitem"
          tabindex="-1"
          v-if="canViewLiveness"
          type="button"
          @click="openDataAction('liveness')"
        >
          <icon-thunderbolt
            class="provider-card-action-icon provider-card-action-icon-liveness"
          />
          <span>测活明细</span>
        </button>
        <button
          role="menuitem"
          tabindex="-1"
          v-if="canViewCheckInRecords"
          type="button"
          @click="openDataAction('checkInRecords')"
        >
          <icon-calendar
            class="provider-card-action-icon provider-card-action-icon-checkin-records"
          />
          <span>签到记录</span>
        </button>
      </div>
    </ProviderCardMenuPopover>

    <ProviderCardMenuPopover
      v-if="hasSiteActions"
      tone="site"
      :label="siteMenuLabel"
      @interaction="menuInteractions.site = $event"
    >
      <template #icon><icon-settings /></template>
      <div class="provider-card-action-panel-title">站点</div>
      <div class="provider-card-action-list">
        <button
          role="menuitem"
          tabindex="-1"
          v-if="canProbeSite"
          type="button"
          :disabled="probingCapabilities"
          @click="openSiteAction('probe')"
        >
          <icon-loading
            v-if="probingCapabilities"
            class="provider-card-action-icon provider-card-action-icon-probe"
          />
          <icon-sync
            v-else
            class="provider-card-action-icon provider-card-action-icon-probe"
          />
          <span>{{ probingCapabilities ? "探测中" : "探测站点能力" }}</span>
        </button>
        <button
          role="menuitem"
          tabindex="-1"
          v-if="showSiteApiKeyManagement"
          type="button"
          @click="openSiteAction('keys')"
        >
          <ProviderAuthIcon
            mode="apiKey"
            class="provider-card-action-icon provider-card-action-icon-keys"
          />
          <span>API Key 管理</span>
        </button>
        <button
          role="menuitem"
          tabindex="-1"
          v-if="canViewAvailableModels"
          type="button"
          @click="openSiteAction('models')"
        >
          <icon-apps
            class="provider-card-action-icon provider-card-action-icon-models"
          />
          <span>可用模型</span>
        </button>
        <button
          role="menuitem"
          tabindex="-1"
          v-if="canChangePassword"
          type="button"
          @click="openSiteAction('password')"
        >
          <icon-lock
            class="provider-card-action-icon provider-card-action-icon-password"
          />
          <span>修改密码</span>
        </button>
      </div>
    </ProviderCardMenuPopover>

    <ProviderCardMenuPopover
      v-if="appImportOptions.length"
      tone="integration"
      label="导出到其他应用"
      panel-class="provider-card-cli-panel"
      @interaction="menuInteractions.apps = $event"
    >
      <template #icon><PackagePlus :size="16" :stroke-width="1.8" /></template>
      <header class="provider-card-cli-panel-header">
        <strong>导出到其他应用</strong>
      </header>
      <p class="provider-card-panel-hint">
        把这张卡的接口地址和 API Key 写入目标应用，由它确认导入。
      </p>
      <ProviderAppImportList
        :options="appImportOptions"
        @select="addProviderToApp"
      />
    </ProviderCardMenuPopover>
  </WorkspaceCardActionGroup>
</template>
