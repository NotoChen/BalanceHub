<script setup lang="ts">
import { computed, ref, watch } from "vue";
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
  IconSwap,
  IconThunderbolt,
} from "@arco-design/web-vue/es/icon";
import { Bot, GitCompareArrows } from "@lucide/vue";
import BrandIcon from "../BrandIcon.vue";
import AgentCliIcon from "../AgentCliIcon.vue";
import ProviderAuthIcon from "../ProviderAuthIcon.vue";
import { useCliRuntimeStore } from "../../stores/cli-runtime";
import type { AgentCliKind, Provider } from "../../stores/providers";
import {
  agentCliLabel,
  availableCliKinds,
} from "../../utils/cli-environment";
import {
  supportsAccountManagement,
  supportsCheckIn,
  supportsInvitation,
} from "../../utils/provider-actions";
import { hasUsableProviderApiKey } from "../../utils/provider-api-key-options";
import {
  providerApiKeyDisplayName,
  providerDefaultApiKeyOption,
} from "../../utils/provider-display";
import {
  canBuildCcSwitchDeeplink,
  ccSwitchTargetLabels,
  ccSwitchTargets,
  type CcSwitchAppTarget,
} from "../../utils/ccswitch-deeplink";
import ccSwitchLogo from "../../assets/logos/cc-switch.png";

const props = withDefaults(
  defineProps<{
    provider: Provider;
    switchingCliKind?: AgentCliKind | null;
    cliConfigSwitching?: boolean;
    probingCapabilities?: boolean;
  }>(),
  {
    switchingCliKind: null,
    cliConfigSwitching: false,
    probingCapabilities: false,
  },
);

const emit = defineEmits<{
  switchCliConfig: [provider: Provider, cliKind: AgentCliKind];
  probeCapabilities: [provider: Provider];
  openApiKeyManager: [provider: Provider];
  openAvailableModels: [provider: Provider];
  openUsage: [provider: Provider];
  openRequestLogs: [provider: Provider];
  openPasswordChange: [provider: Provider];
  openLivenessDetails: [provider: Provider];
  openCheckInRecords: [provider: Provider];
  addCcSwitchConfig: [provider: Provider, target: CcSwitchAppTarget];
  launchTemporaryCli: [provider: Provider];
  copyUrl: [provider: Provider];
  copyInvite: [provider: Provider];
  copySecret: [provider: Provider, field: "apiKey" | "accessToken" | "sessionCookie"];
  interaction: [active: boolean];
}>();

const store = useCliRuntimeStore();
const temporaryCliKinds = computed(() =>
  availableCliKinds(store.cliEnvironmentProbe, "temporaryLaunch"),
);
const configurableCliKinds = computed(() =>
  availableCliKinds(store.cliEnvironmentProbe, "defaultConfig"),
);
const cliSwitchVisible = ref(false);
const copyMenuVisible = ref(false);
const dataMenuVisible = ref(false);
const siteMenuVisible = ref(false);
const ccSwitchMenuVisible = ref(false);
const canSwitchCliConfig = computed(() =>
  Boolean(
    props.provider.identity.baseUrl.trim() &&
      hasUsableProviderApiKey(props.provider.auth.apiKey, props.provider.auth.apiKeyOptions),
  ),
);
const canLaunchTemporaryCli = computed(() =>
  Boolean(
    temporaryCliKinds.value.length > 0 &&
      props.provider.identity.baseUrl.trim() &&
      hasUsableProviderApiKey(props.provider.auth.apiKey, props.provider.auth.apiKeyOptions),
  ),
);
const switchableCliKinds = computed(() => configurableCliKinds.value);
const hasCopyActions = computed(() =>
  Boolean(
    props.provider.identity.baseUrl.trim() ||
      props.provider.auth.apiKey.trim() ||
      props.provider.auth.accessToken.trim() ||
      props.provider.auth.sessionCookie.trim() ||
      (props.provider.runtime.enabled && supportsInvitation(props.provider)),
  ),
);
const canViewAvailableModels = computed(() =>
  Boolean(
    props.provider.auth.apiKey.trim() ||
      (props.provider.capabilities.availableModels || []).length > 0,
  ),
);
const canAddCcSwitchConfig = computed(() => canBuildCcSwitchDeeplink(props.provider));
const accountManagementAvailable = computed(() => supportsAccountManagement(props.provider));
const canViewUsage = computed(
  () => props.provider.runtime.enabled && accountManagementAvailable.value,
);
const canViewRequestLogs = computed(
  () => props.provider.runtime.enabled && accountManagementAvailable.value,
);
const canViewLiveness = computed(
  () => props.provider.liveness.enabled || props.provider.liveness.records.length > 0,
);
const canViewCheckInRecords = computed(
  () =>
    props.provider.auth.mode !== "apiKey" &&
    (supportsCheckIn(props.provider) || props.provider.automation.checkInRecords.length > 0),
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
    props.provider.auth.apiKey.trim()
      || props.provider.auth.apiKeyOptions.length > 0,
  ),
);
const showSiteApiKeyManagement = computed(() =>
  props.provider.auth.mode !== "apiKey"
    && (props.provider.actions.apiKeyManagement || hasManagedApiKeys.value),
);
const hasSiteActions = computed(
  () =>
    canProbeSite.value ||
    showSiteApiKeyManagement.value ||
    canViewAvailableModels.value ||
    canChangePassword.value,
);
const showCliConfigAction = computed(
  () => canSwitchCliConfig.value && switchableCliKinds.value.length > 0,
);
const hasSecondaryActions = computed(
  () =>
    hasCopyActions.value ||
    hasDataActions.value ||
    hasSiteActions.value ||
    showCliConfigAction.value ||
    canAddCcSwitchConfig.value ||
    canLaunchTemporaryCli.value,
);
watch(
  [copyMenuVisible, dataMenuVisible, siteMenuVisible, cliSwitchVisible, ccSwitchMenuVisible],
  (visibleMenus) => emit("interaction", visibleMenus.some(Boolean)),
  { immediate: true },
);

function switchCliConfig(cliKind: AgentCliKind) {
  cliSwitchVisible.value = false;
  if (!props.cliConfigSwitching) {
    emit("switchCliConfig", props.provider, cliKind);
  }
}

function defaultCliKeyLabel(cliKind: AgentCliKind) {
  const snapshot = store.cliRuntime.configs.find(
    (item) => item.cliKind === cliKind && item.providerId === props.provider.identity.id,
  );
  if (!snapshot) return "切换到此中转站";
  const localId = snapshot.apiKeyLocalId?.trim() || "";
  const option = localId
    ? props.provider.auth.apiKeyOptions.find((item) => item.localId.trim() === localId)
    : providerDefaultApiKeyOption(props.provider);
  return option ? `当前绑定：${providerApiKeyDisplayName(option)}` : "当前使用本卡片调用 Key";
}

function openDataAction(action: "usage" | "requestLogs" | "liveness" | "checkInRecords") {
  dataMenuVisible.value = false;
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

function openSiteAction(
  action: "probe" | "keys" | "models" | "password",
) {
  siteMenuVisible.value = false;
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

function addCcSwitchConfig(target: CcSwitchAppTarget) {
  ccSwitchMenuVisible.value = false;
  emit("addCcSwitchConfig", props.provider, target);
}

function launchTemporaryCli() {
  if (canLaunchTemporaryCli.value) {
    emit("launchTemporaryCli", props.provider);
  }
}

function copyProviderUrl() {
  copyMenuVisible.value = false;
  emit("copyUrl", props.provider);
}

function copyProviderInvite() {
  copyMenuVisible.value = false;
  emit("copyInvite", props.provider);
}

function copyProviderSecret(field: "apiKey" | "accessToken" | "sessionCookie") {
  copyMenuVisible.value = false;
  emit("copySecret", props.provider, field);
}

</script>

<template>
<span
  v-if="hasSecondaryActions"
  class="provider-card-action-divider"
  aria-hidden="true"
></span>

<div
  v-if="hasSecondaryActions"
  class="provider-card-action-group provider-card-secondary-actions"
  aria-label="中转站功能"
>
<a-popover
  v-if="hasCopyActions"
  v-model:popup-visible="copyMenuVisible"
  trigger="click"
  position="rt"
  content-class="provider-card-action-popover"
>
  <button
    type="button"
    class="provider-card-icon-action provider-card-copy-action"
    title="复制中转站信息"
    aria-label="复制中转站信息"
    @click.stop
    @pointerdown.stop
  >
    <icon-copy />
  </button>
  <template #content>
    <div class="provider-card-action-panel provider-card-copy-panel" @click.stop @pointerdown.stop>
      <div class="provider-card-action-panel-title">复制</div>
      <div class="provider-card-action-list">
        <button v-if="provider.identity.baseUrl.trim()" type="button" @click="copyProviderUrl">
          <icon-link class="provider-card-action-icon provider-card-action-icon-url" />
          <span>中转站 URL</span>
        </button>
        <button
          v-if="provider.auth.apiKey.trim()"
          type="button"
          @click="copyProviderSecret('apiKey')"
        >
          <ProviderAuthIcon mode="apiKey" />
          <span>API Key</span>
        </button>
        <button
          v-if="provider.auth.mode !== 'apiKey' && provider.auth.accessToken.trim()"
          type="button"
          @click="copyProviderSecret('accessToken')"
        >
          <ProviderAuthIcon mode="accessToken" />
          <span>访问令牌</span>
        </button>
        <button
          v-if="provider.auth.mode !== 'apiKey' && provider.auth.sessionCookie.trim()"
          type="button"
          @click="copyProviderSecret('sessionCookie')"
        >
          <ProviderAuthIcon mode="session" />
          <span>Cookie</span>
        </button>
        <button
          v-if="provider.runtime.enabled && supportsInvitation(provider)"
          type="button"
          @click="copyProviderInvite"
        >
          <icon-link class="provider-card-action-icon provider-card-action-icon-invite" />
          <span>邀请链接</span>
        </button>
      </div>
    </div>
  </template>
</a-popover>

<a-popover
  v-if="hasDataActions"
  v-model:popup-visible="dataMenuVisible"
  trigger="click"
  position="rt"
  content-class="provider-card-action-popover"
>
  <button
    type="button"
    class="provider-card-icon-action provider-card-data-action"
    title="查看中转站数据"
    aria-label="查看中转站数据"
    @click.stop
    @pointerdown.stop
  >
    <icon-bar-chart />
  </button>
  <template #content>
    <div class="provider-card-action-panel" @click.stop @pointerdown.stop>
      <div class="provider-card-action-panel-title">数据</div>
      <div class="provider-card-action-list">
        <button v-if="canViewUsage" type="button" @click="openDataAction('usage')">
          <icon-bar-chart class="provider-card-action-icon provider-card-action-icon-usage" />
          <span>用量趋势</span>
        </button>
        <button v-if="canViewRequestLogs" type="button" @click="openDataAction('requestLogs')">
          <icon-file class="provider-card-action-icon provider-card-action-icon-logs" />
          <span>请求日志</span>
        </button>
        <button v-if="canViewLiveness" type="button" @click="openDataAction('liveness')">
          <icon-thunderbolt class="provider-card-action-icon provider-card-action-icon-liveness" />
          <span>测活明细</span>
        </button>
        <button v-if="canViewCheckInRecords" type="button" @click="openDataAction('checkInRecords')">
          <icon-calendar class="provider-card-action-icon provider-card-action-icon-checkin-records" />
          <span>签到记录</span>
        </button>
      </div>
    </div>
  </template>
</a-popover>

<a-popover
  v-if="hasSiteActions"
  v-model:popup-visible="siteMenuVisible"
  trigger="click"
  position="rt"
  content-class="provider-card-action-popover"
>
  <button
    type="button"
    class="provider-card-icon-action provider-card-site-action"
    title="管理中转站能力"
    aria-label="管理中转站能力"
    @click.stop
    @pointerdown.stop
  >
    <icon-settings />
  </button>
  <template #content>
    <div class="provider-card-action-panel" @click.stop @pointerdown.stop>
      <div class="provider-card-action-panel-title">站点</div>
      <div class="provider-card-action-list">
        <button
          v-if="canProbeSite"
          type="button"
          :disabled="probingCapabilities"
          @click="openSiteAction('probe')"
        >
          <icon-loading
            v-if="probingCapabilities"
            class="provider-card-action-icon provider-card-action-icon-probe"
          />
          <icon-sync v-else class="provider-card-action-icon provider-card-action-icon-probe" />
          <span>{{ probingCapabilities ? "探测中" : "探测站点能力" }}</span>
        </button>
        <button
          v-if="showSiteApiKeyManagement"
          type="button"
          @click="openSiteAction('keys')"
        >
          <ProviderAuthIcon mode="apiKey" class="provider-card-action-icon provider-card-action-icon-keys" />
          <span>API Key 管理</span>
        </button>
        <button
          v-if="canViewAvailableModels"
          type="button"
          @click="openSiteAction('models')"
        >
          <icon-apps class="provider-card-action-icon provider-card-action-icon-models" />
          <span>可用模型</span>
        </button>
        <button
          v-if="canChangePassword"
          type="button"
          @click="openSiteAction('password')"
        >
          <icon-lock class="provider-card-action-icon provider-card-action-icon-password" />
          <span>修改密码</span>
        </button>
      </div>
    </div>
  </template>
</a-popover>

<a-popover
  v-if="showCliConfigAction"
  v-model:popup-visible="cliSwitchVisible"
  trigger="click"
  position="rt"
  content-class="provider-card-action-popover"
>
  <button
    type="button"
    class="provider-card-icon-action provider-card-cli-config-action"
    :disabled="cliConfigSwitching || !canSwitchCliConfig"
    title="预览并切换默认 CLI 配置"
    aria-label="预览并切换默认 CLI 配置"
    @click.stop
    @pointerdown.stop
  >
    <icon-loading
      v-if="switchingCliKind"
      class="provider-card-action-icon provider-card-action-icon-switch"
    />
    <GitCompareArrows v-else :size="16" :stroke-width="1.8" />
  </button>
  <template #content>
    <div class="provider-card-cli-panel" @click.stop @pointerdown.stop>
      <header class="provider-card-cli-panel-header">
        <strong>配置</strong>
      </header>
      <div class="provider-card-action-panel-section-title">默认 CLI</div>
      <div class="provider-card-cli-config-list">
        <button
          v-for="cliKind in switchableCliKinds"
          :key="cliKind"
          type="button"
          :disabled="cliConfigSwitching || !canSwitchCliConfig"
          @click="switchCliConfig(cliKind)"
        >
          <AgentCliIcon :kind="cliKind" :size="16" />
          <span>
            <strong>{{ agentCliLabel(store.cliEnvironmentProbe, cliKind) }}</strong>
            <small>{{ defaultCliKeyLabel(cliKind) }}</small>
          </span>
          <icon-loading
            v-if="switchingCliKind === cliKind"
            class="provider-card-action-icon provider-card-action-icon-switch"
          />
          <icon-swap v-else class="provider-card-action-icon provider-card-action-icon-switch" />
        </button>
      </div>
    </div>
  </template>
</a-popover>

<a-popover
  v-if="canAddCcSwitchConfig"
  v-model:popup-visible="ccSwitchMenuVisible"
  trigger="click"
  position="rt"
  content-class="provider-card-action-popover"
>
  <button
    type="button"
    class="provider-card-icon-action provider-card-ccswitch-action"
    title="添加到 CC Switch"
    aria-label="添加到 CC Switch"
    @click.stop
    @pointerdown.stop
  >
    <img :src="ccSwitchLogo" alt="" aria-hidden="true" />
  </button>
  <template #content>
    <div class="provider-card-cli-panel" @click.stop @pointerdown.stop>
      <header class="provider-card-cli-panel-header">
        <strong>添加到 CC Switch</strong>
      </header>
      <div class="provider-card-cli-config-list">
        <button
          v-for="target in ccSwitchTargets"
          :key="target"
          type="button"
          :disabled="!canAddCcSwitchConfig"
          @click="addCcSwitchConfig(target)"
        >
          <BrandIcon :brand="target" :size="18" />
          <span>
            <strong>导入到 {{ ccSwitchTargetLabels[target] }}</strong>
            <small>仅绑定 URL 与 API Key</small>
          </span>
          <icon-link class="provider-card-action-icon provider-card-action-icon-link" />
        </button>
      </div>
    </div>
  </template>
</a-popover>

<button
  v-if="canLaunchTemporaryCli"
  type="button"
  class="provider-card-icon-action provider-card-launch-action"
  title="启动临时 CLI"
  aria-label="启动临时 CLI"
  @click="launchTemporaryCli"
  @pointerdown.stop
>
  <Bot :size="16" :stroke-width="1.8" />
</button>
</div>
</template>
