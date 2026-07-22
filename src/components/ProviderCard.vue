<script setup lang="ts">
import { computed, ref, type CSSProperties } from "vue";
import {
  IconApps,
  IconBarChart,
  IconCalendar,
  IconCopy,
  IconDelete,
  IconEdit,
  IconFile,
  IconLink,
  IconLoading,
  IconLock,
  IconRefresh,
  IconSafe,
  IconSettings,
  IconSync,
  IconSwap,
  IconThunderbolt,
} from "@arco-design/web-vue/es/icon";
import {
  Bot,
  CalendarCheck2,
  GitCompareArrows,
  Power,
  PowerOff,
} from "@lucide/vue";
import ProviderLivenessTimeline from "./ProviderLivenessTimeline.vue";
import ProviderModelPreview from "./ProviderModelPreview.vue";
import BrandIcon, { type BrandIconName } from "./BrandIcon.vue";
import ProviderAuthIcon from "./ProviderAuthIcon.vue";
import type { LivenessCliKind, Provider } from "../stores/providers";
import {
  availablePercent,
  availablePercentLabel,
  providerAvailableQuotaLabel,
  providerCheckedInToday,
  providerAuthModeDescription,
  providerIdentityDisplayName,
  providerIdentityId,
  providerIdentitySecondaryUsername,
  providerIdentityUsername,
  providerQuotaScopeLabel,
  providerQuotaUnlimited,
  supportsApiKeyManagement,
  supportsAccountManagement,
  supportsCheckIn,
  supportsInvitation,
  type ProviderCardTone,
} from "../utils/provider-display";
import {
  canBuildCcSwitchDeeplink,
  ccSwitchTargetLabels,
  ccSwitchTargets,
  type CcSwitchAppTarget,
} from "../utils/ccswitch-deeplink";
import newApiLogo from "../assets/logos/new-api.png";
import ccSwitchLogo from "../assets/logos/cc-switch.png";

const props = withDefaults(
  defineProps<{
    provider: Provider;
    tone: ProviderCardTone;
    title?: string;
    interactive?: boolean;
    placeholder?: boolean;
    dragOver?: boolean;
    dragging?: boolean;
    dragStyle?: CSSProperties;
    showLivenessTimeline?: boolean;
    codexDefault?: boolean;
    claudeDefault?: boolean;
    activeCliCount?: number;
    switchingCliKind?: LivenessCliKind | null;
    cliConfigSwitching?: boolean;
    probingCapabilities?: boolean;
    checkingIn?: boolean;
    ariaHidden?: boolean;
  }>(),
  {
    title: "",
    interactive: true,
    placeholder: false,
    dragOver: false,
    dragging: false,
    dragStyle: undefined,
    showLivenessTimeline: false,
    codexDefault: false,
    claudeDefault: false,
    activeCliCount: 0,
    switchingCliKind: null,
    cliConfigSwitching: false,
    probingCapabilities: false,
    checkingIn: false,
    ariaHidden: false,
  },
);

const emit = defineEmits<{
  click: [provider: Provider, event: MouseEvent];
  pointerdown: [provider: Provider, event: PointerEvent];
  enter: [provider: Provider, event: KeyboardEvent];
  openCliInstances: [provider: Provider];
  switchCliConfig: [provider: Provider, cliKind: LivenessCliKind];
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
  edit: [provider: Provider];
  toggle: [provider: Provider];
  refresh: [provider: Provider];
  checkIn: [provider: Provider];
  remove: [provider: Provider];
}>();

const toneLabels: Record<Exclude<ProviderCardTone, "disabled">, string> = {
  ok: "正常",
  pending: "待同步",
  syncing: "同步中",
  warning: "待签到",
  empty: "无余额",
  error: "异常",
};

const authModeDescription = computed(() => providerAuthModeDescription(props.provider));
const cliSwitchVisible = ref(false);
const copyMenuVisible = ref(false);
const dataMenuVisible = ref(false);
const siteMenuVisible = ref(false);
const ccSwitchMenuVisible = ref(false);
const identityDisplayName = computed(
  () => providerIdentityDisplayName(props.provider) || providerIdentityUsername(props.provider),
);
const identityUsername = computed(() => providerIdentitySecondaryUsername(props.provider));
const identityId = computed(() => providerIdentityId(props.provider));
const quotaTone = computed(() => {
  if (providerQuotaUnlimited(props.provider)) {
    return "unlimited";
  }
  if (!props.provider.automation.lastSyncedAt) {
    return "neutral";
  }
  const percent = availablePercent(props.provider);
  if (props.provider.quota.available <= 0 || percent <= 0) {
    return "empty";
  }
  return percent <= 0.2 ? "warning" : "normal";
});
const canSwitchCliConfig = computed(() =>
  Boolean(props.provider.identity.baseUrl.trim() && props.provider.auth.apiKey.trim()),
);
const canLaunchTemporaryCli = computed(() =>
  Boolean(
    props.provider.identity.baseUrl.trim() &&
      (props.provider.auth.apiKey.trim() || supportsApiKeyManagement(props.provider)),
  ),
);
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
const canViewUsage = computed(() => props.provider.runtime.enabled && accountManagementAvailable.value);
const canViewRequestLogs = computed(() => props.provider.runtime.enabled && accountManagementAvailable.value);
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
const canProbeSite = computed(() => props.provider.runtime.enabled && accountManagementAvailable.value);
const canChangePassword = computed(() => accountManagementAvailable.value);
const hasSiteActions = computed(
  () =>
    canProbeSite.value ||
    supportsApiKeyManagement(props.provider) ||
    canViewAvailableModels.value ||
    canChangePassword.value,
);
const showCliConfigAction = computed(
  () => canSwitchCliConfig.value && !(props.codexDefault && props.claudeDefault),
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
const canCheckInAction = computed(() => props.provider.runtime.enabled && supportsCheckIn(props.provider));
const checkedInToday = computed(() => providerCheckedInToday(props.provider));

function ccSwitchTargetBrand(target: CcSwitchAppTarget): BrandIconName {
  return target === "claude" ? "claude" : target;
}

function providerStatusLabel() {
  if (props.tone === "disabled") {
    return "已停用";
  }
  return toneLabels[props.tone];
}

function providerLogoSrc(provider: Provider) {
  return provider.identity.siteLogo?.trim() || newApiLogo;
}

function handleProviderLogoError(event: Event) {
  const image = event.target as HTMLImageElement;
  if (image.src !== newApiLogo) {
    image.src = newApiLogo;
  }
}

function handleClick(event: MouseEvent) {
  if (props.interactive) {
    emit("click", props.provider, event);
  }
}

function handlePointerDown(event: PointerEvent) {
  if (props.interactive) {
    emit("pointerdown", props.provider, event);
  }
}

function handleEnter(event: KeyboardEvent) {
  if (props.interactive) {
    emit("enter", props.provider, event);
  }
}

function openCliInstances() {
  emit("openCliInstances", props.provider);
}

function switchCliConfig(cliKind: LivenessCliKind) {
  const isCurrent = cliKind === "codex" ? props.codexDefault : props.claudeDefault;
  cliSwitchVisible.value = false;
  if (!isCurrent && !props.cliConfigSwitching) {
    emit("switchCliConfig", props.provider, cliKind);
  }
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

function openSiteAction(action: "probe" | "keys" | "models" | "password") {
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

function editProvider() {
  emit("edit", props.provider);
}

function toggleProvider() {
  emit("toggle", props.provider);
}

function refreshProvider() {
  if (props.provider.runtime.enabled) {
    emit("refresh", props.provider);
  }
}

function checkInProvider() {
  if (canCheckInAction.value && !checkedInToday.value && !props.checkingIn) {
    emit("checkIn", props.provider);
  }
}

function removeProvider() {
  emit("remove", props.provider);
}
</script>

<template>
  <article
    :data-provider-id="provider.identity.id"
    class="provider-card"
    :class="[
      `provider-card-${tone}`,
      {
        'provider-card-disabled': tone === 'disabled',
        'provider-card-placeholder': placeholder,
        'provider-card-drag-over': dragOver,
        'provider-card-dragging': dragging,
      },
    ]"
    :role="interactive ? 'group' : undefined"
    :aria-disabled="interactive ? !provider.runtime.enabled : undefined"
    :aria-hidden="ariaHidden || undefined"
    :aria-label="interactive ? `${provider.identity.name} 中转站卡片` : undefined"
    :tabindex="interactive ? 0 : undefined"
    :title="title"
    :style="dragStyle"
    @click="handleClick"
    @dragstart.prevent
    @pointerdown="handlePointerDown"
    @keydown.enter="handleEnter"
  >
    <header class="provider-card-header">
      <div class="provider-card-brand">
        <div class="provider-logo provider-card-logo">
          <img
            :src="providerLogoSrc(provider)"
            :alt="provider.identity.name"
            draggable="false"
            @error="handleProviderLogoError"
          />
        </div>
        <div class="provider-card-brand-copy">
          <h3 class="provider-card-title">{{ provider.identity.name }}</h3>
          <span class="provider-card-type">NewAPI</span>
        </div>
      </div>
      <div class="provider-card-header-meta">
        <div v-if="codexDefault || claudeDefault" class="provider-card-default-signals" aria-label="默认 CLI 配置">
          <span
            v-if="codexDefault"
            class="provider-card-default-signal"
            title="Codex 当前使用此中转站"
            aria-label="Codex 当前使用此中转站"
          >
            <BrandIcon brand="codex" :size="14" />
          </span>
          <span
            v-if="claudeDefault"
            class="provider-card-default-signal"
            title="Claude Code 当前使用此中转站"
            aria-label="Claude Code 当前使用此中转站"
          >
            <BrandIcon brand="claude" :size="14" />
          </span>
        </div>
        <div class="provider-card-status" :title="title">
          <i aria-hidden="true"></i>
          <span>{{ providerStatusLabel() }}</span>
        </div>
      </div>
    </header>

    <div class="provider-card-content">
      <section class="provider-card-identity" aria-label="账号信息">
        <strong
          v-if="identityDisplayName"
          class="provider-card-user-name"
          :title="identityDisplayName"
        >
          {{ identityDisplayName }}
        </strong>
        <span v-else class="provider-card-user-name provider-card-user-name-muted">
          用户信息未同步
        </span>
        <div v-if="identityUsername || identityId" class="provider-card-user-meta">
          <span v-if="identityUsername" :title="identityUsername">{{ identityUsername }}</span>
          <span v-if="identityId" :title="identityId">{{ identityId }}</span>
        </div>
      </section>

      <section
        class="provider-card-quota"
        :class="`provider-card-quota-${quotaTone}`"
        aria-label="账户余额"
      >
        <div class="provider-card-balance">
          <span>{{ providerQuotaScopeLabel(provider) }}</span>
          <strong :title="providerAvailableQuotaLabel(provider)">
            {{ providerAvailableQuotaLabel(provider) }}
          </strong>
        </div>
        <div v-if="!providerQuotaUnlimited(provider)" class="provider-card-progress-row">
          <span>可用 {{ availablePercentLabel(provider) }}</span>
          <a-progress
            class="provider-quota-progress"
            :percent="availablePercent(provider)"
            :show-text="false"
            size="small"
          />
        </div>
        <div v-else class="provider-card-unlimited">无限额度</div>
      </section>

      <ProviderModelPreview :models="provider.capabilities.availableModels" />

      <ProviderLivenessTimeline
        v-if="showLivenessTimeline"
        :records="provider.liveness.records"
      />

      <footer class="provider-card-footer" @click.stop @pointerdown.stop>
        <div class="provider-card-footer-meta">
          <span
            class="provider-card-auth-summary"
            :title="authModeDescription"
            :aria-label="authModeDescription"
          >
            <ProviderAuthIcon :mode="provider.auth.mode" />
          </span>
          <button
            v-if="activeCliCount > 0 && interactive"
            type="button"
            class="provider-card-cli-instance-trigger"
            :title="`查看 ${activeCliCount} 个活动 CLI`"
            :aria-label="`查看 ${activeCliCount} 个活动 CLI`"
            @click="openCliInstances"
            @pointerdown.stop
          >
            <Bot :size="15" :stroke-width="1.8" />
            <strong>{{ activeCliCount }}</strong>
          </button>
          <span v-else-if="activeCliCount > 0" class="provider-card-cli-instance-trigger">
            <Bot :size="15" :stroke-width="1.8" />
            <strong>{{ activeCliCount }}</strong>
          </span>
        </div>

        <div v-if="interactive" class="provider-card-quick-actions" aria-label="快捷操作">
          <div class="provider-card-action-group provider-card-primary-actions" aria-label="中转站管理">
            <button
              v-if="canCheckInAction"
              type="button"
              class="provider-card-icon-action provider-card-checkin-action"
              :disabled="checkedInToday || checkingIn"
              :title="checkingIn ? '签到中' : checkedInToday ? '今日已签到' : '签到'"
              :aria-label="checkingIn ? '签到中' : checkedInToday ? '今日已签到' : '签到'"
              @click="checkInProvider"
              @pointerdown.stop
            >
              <icon-loading v-if="checkingIn" />
              <CalendarCheck2 v-else :size="15" :stroke-width="1.9" />
            </button>

            <button
              type="button"
              class="provider-card-icon-action provider-card-refresh-action"
              :disabled="!provider.runtime.enabled"
              :title="provider.runtime.enabled ? '刷新额度' : '中转站已停用，无法刷新'"
              aria-label="刷新额度"
              @click="refreshProvider"
              @pointerdown.stop
            >
              <icon-refresh />
            </button>

            <button
              type="button"
              class="provider-card-icon-action provider-card-edit-action"
              title="编辑中转站"
              aria-label="编辑中转站"
              @click="editProvider"
              @pointerdown.stop
            >
              <icon-edit />
            </button>

            <button
              type="button"
              class="provider-card-icon-action provider-card-toggle-action"
              :class="{ 'provider-card-enable-action': !provider.runtime.enabled }"
              :title="provider.runtime.enabled ? '停用中转站' : '启用中转站'"
              :aria-label="provider.runtime.enabled ? '停用中转站' : '启用中转站'"
              @click="toggleProvider"
              @pointerdown.stop
            >
              <PowerOff v-if="provider.runtime.enabled" :size="15" :stroke-width="1.9" />
              <Power v-else :size="15" :stroke-width="1.9" />
            </button>

            <button
              type="button"
              class="provider-card-icon-action provider-card-delete-action"
              title="删除中转站"
              aria-label="删除中转站"
              @click="removeProvider"
              @pointerdown.stop
            >
              <icon-delete />
            </button>
          </div>

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
                    v-if="supportsApiKeyManagement(provider)"
                    type="button"
                    @click="openSiteAction('keys')"
                  >
                    <icon-safe class="provider-card-action-icon provider-card-action-icon-keys" />
                    <span>密钥管理</span>
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
                    v-if="!codexDefault"
                    type="button"
                    :disabled="cliConfigSwitching || !canSwitchCliConfig"
                    @click="switchCliConfig('codex')"
                  >
                    <BrandIcon brand="codex" :size="16" />
                    <span>
                      <strong>Codex</strong>
                      <small>切换到此中转站</small>
                    </span>
                    <icon-loading
                      v-if="switchingCliKind === 'codex'"
                      class="provider-card-action-icon provider-card-action-icon-switch"
                    />
                    <icon-swap v-else class="provider-card-action-icon provider-card-action-icon-switch" />
                  </button>
                  <button
                    v-if="!claudeDefault"
                    type="button"
                    :disabled="cliConfigSwitching || !canSwitchCliConfig"
                    @click="switchCliConfig('claudeCode')"
                  >
                    <BrandIcon brand="claude" :size="16" />
                    <span>
                      <strong>Claude Code</strong>
                      <small>切换到此中转站</small>
                    </span>
                    <icon-loading
                      v-if="switchingCliKind === 'claudeCode'"
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
                    <BrandIcon :brand="ccSwitchTargetBrand(target)" :size="16" />
                    <span>
                      <strong>{{ ccSwitchTargetLabels[target] }}</strong>
                      <small>添加此中转站配置</small>
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
        </div>
      </footer>
    </div>
  </article>
</template>
