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
import BrandIcon from "./BrandIcon.vue";
import ProviderAuthIcon from "./ProviderAuthIcon.vue";
import { useProviderStore, type LivenessCliKind, type Provider } from "../stores/providers";
import { availableCliKinds, cliKindMeta } from "../utils/cli-environment";
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
  providerQuotaKnown,
  providerQuotaTotalKnown,
  providerProtocolLabel,
  providerQuotaUnlimited,
  maskApiKey,
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
import sub2ApiLogo from "../assets/logos/sub2api.svg";
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
    codexActiveCliCount?: number;
    claudeActiveCliCount?: number;
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
    codexActiveCliCount: 0,
    claudeActiveCliCount: 0,
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
  openCliInstances: [provider: Provider, cliKind: LivenessCliKind];
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

const store = useProviderStore();
const detectedCliKinds = computed(() => availableCliKinds(store.cliEnvironmentProbe));
const codexDetected = computed(() => detectedCliKinds.value.includes("codex"));
const claudeDetected = computed(() => detectedCliKinds.value.includes("claudeCode"));

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
const isApiKeyAuth = computed(() => props.provider.auth.mode === "apiKey");
const isGenericApi = computed(() => props.provider.identity.protocol === "api");
const identityEmptyLabel = computed(() =>
  props.provider.identity.protocol === "api" ? "通用模型接口" : "用户信息未同步",
);
const apiKeyMasked = computed(() => maskApiKey(props.provider.auth.apiKey));
const apiKeyConfigured = computed(() => Boolean(props.provider.auth.apiKey.trim()));
const providerUrlDisplay = computed(() =>
  props.provider.identity.baseUrl
    .trim()
    .replace(/^https?:\/\//i, "")
    .replace(/\/+$/, ""),
);
const providerHeaderTitle = computed(() => props.provider.identity.name);
const providerHeaderSubtitle = computed(() => providerProtocolLabel(props.provider.identity.protocol));
const showProviderStatus = computed(() => !isApiKeyAuth.value || props.tone !== "ok");
const refreshActionTitle = computed(() =>
  props.provider.identity.protocol === "api" ? "刷新模型列表" : "刷新额度",
);
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
    detectedCliKinds.value.length > 0 &&
    props.provider.identity.baseUrl.trim() &&
      (props.provider.auth.apiKey.trim() || supportsApiKeyManagement(props.provider)),
  ),
);
const switchableCliKinds = computed(() =>
  detectedCliKinds.value.filter(
    (kind) => !(kind === "codex" ? props.codexDefault : props.claudeDefault),
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
const canCheckInAction = computed(() => props.provider.runtime.enabled && supportsCheckIn(props.provider));
const checkedInToday = computed(() => providerCheckedInToday(props.provider));

function providerStatusLabel() {
  if (props.tone === "disabled") {
    return "已停用";
  }
  return toneLabels[props.tone];
}

function providerLogoSrc(provider: Provider) {
  if (provider.identity.siteLogo?.trim()) {
    return provider.identity.siteLogo;
  }
  if (provider.identity.protocol === "sub2Api") {
    return sub2ApiLogo;
  }
  return newApiLogo;
}

function handleProviderLogoError(event: Event) {
  const image = event.target as HTMLImageElement;
  const fallback = props.provider.identity.protocol === "sub2Api" ? sub2ApiLogo : newApiLogo;
  if (image.src !== fallback) {
    image.src = fallback;
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

function openCliInstances(cliKind: LivenessCliKind) {
  emit("openCliInstances", props.provider, cliKind);
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
        'provider-card-api-key': isApiKeyAuth,
        'provider-card-generic-api': isGenericApi,
        'provider-card-standard': !showLivenessTimeline,
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
      <dl v-if="isApiKeyAuth" class="provider-card-api-summary" aria-label="API Key 信息">
        <div class="provider-card-api-field">
          <dt>接口地址</dt>
          <dd
            v-if="provider.identity.baseUrl.trim()"
            class="provider-card-api-endpoint-value"
            :title="provider.identity.baseUrl"
          >
            {{ providerUrlDisplay }}
          </dd>
          <dd v-else class="provider-card-api-value-muted">未配置</dd>
        </div>
        <div class="provider-card-api-field">
          <dt>API Key</dt>
          <dd class="provider-card-api-key-value">
            <code v-if="apiKeyConfigured" :title="`API Key：${apiKeyMasked}`">
              {{ apiKeyMasked }}
            </code>
            <span v-else class="provider-card-api-value-muted">未配置</span>
          </dd>
        </div>
      </dl>
      <div v-else class="provider-card-brand">
        <div class="provider-logo provider-card-logo">
          <img
            :src="providerLogoSrc(provider)"
            :alt="provider.identity.name"
            draggable="false"
            @error="handleProviderLogoError"
          />
        </div>
        <div class="provider-card-brand-copy">
          <h3 class="provider-card-title" :title="providerHeaderTitle">
            {{ providerHeaderTitle }}
          </h3>
          <span class="provider-card-type">
            {{ providerHeaderSubtitle }}
          </span>
        </div>
      </div>
      <div class="provider-card-header-meta">
        <div
          v-if="(codexDefault && codexDetected) || (claudeDefault && claudeDetected) || codexActiveCliCount > 0 || claudeActiveCliCount > 0"
          class="provider-card-cli-signals"
          :class="{ 'provider-card-cli-signals-standalone': !showProviderStatus }"
          aria-label="CLI 使用状态"
        >
          <span
            v-if="codexDefault && codexDetected"
            class="provider-card-cli-signal provider-card-cli-signal-default"
            title="Codex 默认 CLI 配置"
            aria-label="Codex 默认 CLI 配置"
          >
            <BrandIcon brand="codex" :size="17" />
            <b>D</b>
          </span>
          <span
            v-if="claudeDefault && claudeDetected"
            class="provider-card-cli-signal provider-card-cli-signal-default"
            title="Claude Code 默认 CLI 配置"
            aria-label="Claude Code 默认 CLI 配置"
          >
            <BrandIcon brand="claude" :size="17" />
            <b>D</b>
          </span>
          <button
            v-if="codexActiveCliCount > 0 && interactive"
            type="button"
            class="provider-card-cli-signal provider-card-cli-signal-active"
            :title="`查看 ${codexActiveCliCount} 个 Codex 临时 CLI`"
            :aria-label="`查看 ${codexActiveCliCount} 个 Codex 临时 CLI`"
            @click.stop="openCliInstances('codex')"
            @pointerdown.stop
          >
            <BrandIcon brand="codex" :size="17" />
            <b>{{ codexActiveCliCount }}</b>
          </button>
          <span
            v-else-if="codexActiveCliCount > 0"
            class="provider-card-cli-signal provider-card-cli-signal-active"
            :title="`${codexActiveCliCount} 个 Codex 临时 CLI`"
            :aria-label="`${codexActiveCliCount} 个 Codex 临时 CLI`"
          >
            <BrandIcon brand="codex" :size="17" />
            <b>{{ codexActiveCliCount }}</b>
          </span>
          <button
            v-if="claudeActiveCliCount > 0 && interactive"
            type="button"
            class="provider-card-cli-signal provider-card-cli-signal-active"
            :title="`查看 ${claudeActiveCliCount} 个 Claude Code 临时 CLI`"
            :aria-label="`查看 ${claudeActiveCliCount} 个 Claude Code 临时 CLI`"
            @click.stop="openCliInstances('claudeCode')"
            @pointerdown.stop
          >
            <BrandIcon brand="claude" :size="17" />
            <b>{{ claudeActiveCliCount }}</b>
          </button>
          <span
            v-else-if="claudeActiveCliCount > 0"
            class="provider-card-cli-signal provider-card-cli-signal-active"
            :title="`${claudeActiveCliCount} 个 Claude Code 临时 CLI`"
            :aria-label="`${claudeActiveCliCount} 个 Claude Code 临时 CLI`"
          >
            <BrandIcon brand="claude" :size="17" />
            <b>{{ claudeActiveCliCount }}</b>
          </span>
        </div>
        <div v-if="showProviderStatus" class="provider-card-status" :title="title">
          <i aria-hidden="true"></i>
          <span>{{ providerStatusLabel() }}</span>
        </div>
      </div>
    </header>

    <div class="provider-card-content">
      <section
        v-if="!isApiKeyAuth"
        class="provider-card-identity"
        aria-label="账号信息"
      >
        <strong
          v-if="identityDisplayName"
          class="provider-card-user-name"
          :title="identityDisplayName"
        >
          {{ identityDisplayName }}
        </strong>
        <span v-else class="provider-card-user-name provider-card-user-name-muted">
          {{ identityEmptyLabel }}
        </span>
        <div v-if="identityUsername || identityId" class="provider-card-user-meta">
          <span v-if="identityUsername" :title="identityUsername">{{ identityUsername }}</span>
          <span v-if="identityId" :title="identityId">{{ identityId }}</span>
        </div>
      </section>

      <section
        v-if="!isApiKeyAuth"
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
        <div v-if="providerQuotaKnown(provider) && providerQuotaTotalKnown(provider) && !providerQuotaUnlimited(provider)" class="provider-card-progress-row">
          <span>可用 {{ availablePercentLabel(provider) }}</span>
          <a-progress
            class="provider-quota-progress"
            :percent="availablePercent(provider)"
            :show-text="false"
            size="small"
          />
        </div>
        <div v-else-if="providerQuotaUnlimited(provider)" class="provider-card-unlimited">无限额度</div>
        <div v-else class="provider-card-unknown">额度未公开</div>
      </section>

      <ProviderModelPreview
        :models="provider.capabilities.availableModels"
        :rows="isApiKeyAuth ? 5 : 2"
      />

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
            <ProviderAuthIcon :mode="provider.auth.mode" :protocol="provider.identity.protocol" />
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
              :title="provider.runtime.enabled ? refreshActionTitle : '中转站已停用，无法刷新'"
              :aria-label="refreshActionTitle"
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
                    v-for="cliKind in switchableCliKinds"
                    :key="cliKind"
                    type="button"
                    :disabled="cliConfigSwitching || !canSwitchCliConfig"
                    @click="switchCliConfig(cliKind)"
                  >
                    <BrandIcon :brand="cliKindMeta[cliKind].brand" :size="16" />
                    <span>
                      <strong>{{ cliKindMeta[cliKind].label }}</strong>
                      <small>切换到此中转站</small>
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
                    :class="{ 'provider-card-cli-config-item-no-logo': isApiKeyAuth }"
                    :disabled="!canAddCcSwitchConfig"
                    @click="addCcSwitchConfig(target)"
                  >
                    <img
                      v-if="!isApiKeyAuth"
                      class="provider-card-ccswitch-provider-icon"
                      :src="providerLogoSrc(provider)"
                      alt=""
                      aria-hidden="true"
                      @error="handleProviderLogoError"
                    />
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
        </div>
      </footer>
    </div>
  </article>
</template>
