<script setup lang="ts">
import { computed } from "vue";
import {
  ArrowLeftRight,
  Power,
  PowerOff,
  RotateCcw,
  SquareTerminal,
} from "@lucide/vue";
import {
  IconDelete,
  IconEdit,
  IconLoading,
  IconRefresh,
} from "@arco-design/web-vue/es/icon";
import AgentCliIcon from "../AgentCliIcon.vue";
import ProviderAuthIcon from "../ProviderAuthIcon.vue";
import CardIconButton from "../workspace-card/CardIconButton.vue";
import WorkspaceCardActionGroup from "../workspace-card/WorkspaceCardActionGroup.vue";
import WorkspaceCardFooter from "../workspace-card/WorkspaceCardFooter.vue";
import ProviderCardActionMenus from "./ProviderCardActionMenus.vue";
import ProviderCheckInAction from "./ProviderCheckInAction.vue";
import { useCliRuntimeStore } from "../../stores/cli-runtime";
import type { AgentCliKind, Provider } from "../../stores/providers";
import type { ProviderAppImportTarget } from "../../stores/provider-types";
import { agentCliLabel, availableCliKinds } from "../../utils/cli-environment";
import { oauthAgentCliKind } from "../../utils/oauth-cli";
import {
  providerApiKeyDisplayName,
  providerAuthModeDescription,
  providerCardScopeNoun,
  providerDefaultApiKeyOption,
} from "../../utils/provider-display";
import {
  providerCheckedInToday,
  supportsCheckIn,
} from "../../utils/provider-actions";

/// 四类卡片共用同一排底栏：日常动作（启动 CLI、签到、刷新、切换授权）在前，
/// 管理动作（全部续期、重新授权、编辑、启停、删除）在后。卡型只决定哪些动作
/// 有数据，不决定它们长在哪、叫什么。
const props = withDefaults(
  defineProps<{
    provider: Provider;
    interactive?: boolean;
    switchingCliKind?: AgentCliKind | null;
    cliConfigSwitching?: boolean;
    probingCapabilities?: boolean;
    checkingIn?: boolean;
  }>(),
  {
    interactive: true,
    switchingCliKind: null,
    cliConfigSwitching: false,
    probingCapabilities: false,
    checkingIn: false,
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
  addProviderToApp: [provider: Provider, target: ProviderAppImportTarget];
  launchTemporaryCli: [provider: Provider];
  copyUrl: [provider: Provider];
  copyInvite: [provider: Provider];
  copySecret: [
    provider: Provider,
    field: "apiKey" | "accessToken" | "sessionCookie",
  ];
  edit: [provider: Provider];
  toggle: [provider: Provider];
  refresh: [provider: Provider];
  checkIn: [provider: Provider];
  remove: [provider: Provider];
  cpaRenewAll: [provider: Provider];
  reauthorizeOauthCard: [provider: Provider];
  interaction: [active: boolean];
}>();

const store = useCliRuntimeStore();
/// 底栏、菜单和提示对这张卡片的称呼只有一份：中转站 / 账号 / 网关。
const noun = computed(() => providerCardScopeNoun(props.provider));
const oauth = computed(() => props.provider.auth.oauth);
const authModeDescription = computed(() =>
  providerAuthModeDescription(props.provider),
);
const syncing = computed(() => props.provider.runtime.status === "syncing");
const canRefresh = computed(
  () => props.provider.runtime.enabled && !syncing.value,
);
const refreshTitle = computed(() => {
  if (!props.provider.runtime.enabled) return `${noun.value}已停用，无法刷新`;
  if (syncing.value) return "正在同步";
  return props.provider.actions.refreshModelsOnly
    ? "刷新模型列表"
    : "刷新额度与模型";
});
const canCheckIn = computed(
  () => props.provider.runtime.enabled && supportsCheckIn(props.provider),
);
const checkedInToday = computed(() => providerCheckedInToday(props.provider));
/// 换一个授权目标每天都会发生，和刷新同一排；卡片有没有凭据由后端能力决定。
const canLaunchCli = computed(() => props.provider.actions.temporaryCli);
const launchCliKind = computed<AgentCliKind | null>(() =>
  oauth.value ? oauthAgentCliKind(oauth.value.vendor) : null,
);
const launchTitle = computed(() =>
  launchCliKind.value
    ? `启动 ${agentCliLabel(store.cliEnvironmentProbe, launchCliKind.value)} 临时 CLI`
    : "启动临时 CLI",
);
/// A broken renewal cannot be fixed from the panel's fields, only by running
/// the vendor login again, so the remedy appears next to the management
/// actions only while it is actually needed.
const renewal = computed(() => {
  const account = oauth.value;
  if (!account) return { label: "", tone: "", detail: "" };
  if (account.refreshError) {
    return { label: "续期失败", tone: "is-error", detail: account.refreshError };
  }
  if (account.refreshPending) {
    return {
      label: "续期待确认",
      tone: "is-warning",
      detail: "上次续期未得到明确结果，已暂停自动续期，请重新授权",
    };
  }
  return { label: "", tone: "", detail: "" };
});
const renewalTitle = computed(() =>
  renewal.value.detail
    ? `${oauth.value ? "OAuth" : "CPA 网关"} · ${renewal.value.detail}`
    : "",
);
const reauthRecommended = computed(() => renewal.value.label !== "");
const reauthTitle = computed(
  () =>
    `${renewal.value.label}，重新执行 ${oauth.value ? "OAuth" : "网关"} 授权`,
);
/// Which agents currently point at this card. That is the one fact the card
/// cannot show from its own data, and it is what the switch action changes.
const boundKinds = computed<AgentCliKind[]>(() =>
  store.cliRuntime.configs
    .filter((item) => item.providerId === props.provider.identity.id)
    .map((item) => item.cliKind),
);
const bound = computed(() => boundKinds.value[0] ?? null);
const boundLabel = computed(() =>
  bound.value ? agentCliLabel(store.cliEnvironmentProbe, bound.value) : "",
);
const boundTitle = computed(() =>
  boundKinds.value.length
    ? `${boundKinds.value
        .map((kind) => agentCliLabel(store.cliEnvironmentProbe, kind))
        .join("、")} 正在使用此${noun.value}的授权`
    : "",
);
/// The card may only write its own credential into the agent that credential
/// belongs to, so an OAuth card offers exactly one target.
const switchableKinds = computed(() => {
  const available = availableCliKinds(
    store.cliEnvironmentProbe,
    "defaultConfig",
  );
  const vendor = oauth.value ? oauthAgentCliKind(oauth.value.vendor) : null;
  return vendor ? available.filter((kind) => kind === vendor) : available;
});
const switchControlVisible = computed(
  () =>
    props.provider.actions.defaultCliConfig &&
    switchableKinds.value.length > 0,
);
const canSwitchCliConfig = computed(
  () => switchControlVisible.value && !props.cliConfigSwitching,
);
/// 底栏的切换按钮已经用 Agent 图标和 `is-serving` 标出「谁在用这张卡的授权」，
/// 同一行再放一枚绑定胶囊只是把这一行挤成两行。只有底栏没有切换入口时，
/// 绑定才需要靠胶囊说。
const boundPill = computed(() => (switchControlVisible.value ? null : bound.value));
/// An OAuth credential belongs to exactly one vendor, so the switch is a
/// single click there. Only a gateway can serve several agents at once, and
/// only that case needs a menu.
const singleSwitchKind = computed(() =>
  switchableKinds.value.length === 1 ? switchableKinds.value[0] : null,
);
const switchTitle = computed(() => {
  if (!switchableKinds.value.length) return "";
  const serving = switchableKinds.value
    .filter(isServing)
    .map((kind) => agentCliLabel(store.cliEnvironmentProbe, kind));
  return serving.length
    ? `${serving.join("、")} 正在使用此${noun.value}的授权`
    : `把 ${switchableKinds.value
        .map((kind) => agentCliLabel(store.cliEnvironmentProbe, kind))
        .join("、")} 的默认授权切换到此处`;
});

function isServing(kind: AgentCliKind) {
  return store.cliRuntime.configs.some(
    (item) =>
      item.cliKind === kind && item.providerId === props.provider.identity.id,
  );
}

/// 绑定到某个 Agent 的 Key 名：中转站卡片换的是 Key，账号卡片换的是授权，
/// 有 Key 名时用户才知道切换后调用哪一把。
function boundKeyName(kind: AgentCliKind) {
  const snapshot = store.cliRuntime.configs.find(
    (item) =>
      item.cliKind === kind && item.providerId === props.provider.identity.id,
  );
  if (!snapshot) return "";
  const localId = snapshot.apiKeyLocalId?.trim() || "";
  const option = localId
    ? props.provider.auth.apiKeyOptions.find(
        (item) => item.localId.trim() === localId,
      )
    : providerDefaultApiKeyOption(props.provider);
  return option ? providerApiKeyDisplayName(option) : "";
}

function cliOptionHint(kind: AgentCliKind) {
  if (!isServing(kind)) return `切换到此${noun.value}`;
  const keyName = boundKeyName(kind);
  return keyName
    ? `当前使用此${noun.value} · ${keyName}`
    : `当前使用此${noun.value}授权`;
}

function switchCliConfig(kind: AgentCliKind) {
  if (!props.cliConfigSwitching) emit("switchCliConfig", props.provider, kind);
}

function editProvider() {
  emit("edit", props.provider);
}

function toggleProvider() {
  emit("toggle", props.provider);
}

function refreshProvider() {
  if (canRefresh.value) emit("refresh", props.provider);
}

function checkInProvider() {
  if (canCheckIn.value && !checkedInToday.value && !props.checkingIn) {
    emit("checkIn", props.provider);
  }
}

function removeProvider() {
  emit("remove", props.provider);
}

function launchTemporaryCli() {
  if (canLaunchCli.value) emit("launchTemporaryCli", props.provider);
}

const menuListeners = {
  probeCapabilities: (provider: Provider) =>
    emit("probeCapabilities", provider),
  openApiKeyManager: (provider: Provider) =>
    emit("openApiKeyManager", provider),
  openAvailableModels: (provider: Provider) =>
    emit("openAvailableModels", provider),
  openUsage: (provider: Provider) => emit("openUsage", provider),
  openRequestLogs: (provider: Provider) => emit("openRequestLogs", provider),
  openPasswordChange: (provider: Provider) =>
    emit("openPasswordChange", provider),
  openLivenessDetails: (provider: Provider) =>
    emit("openLivenessDetails", provider),
  openCheckInRecords: (provider: Provider) =>
    emit("openCheckInRecords", provider),
  addProviderToApp: (
    provider: Provider,
    target: ProviderAppImportTarget,
  ) => emit("addProviderToApp", provider, target),
  copyUrl: (provider: Provider) => emit("copyUrl", provider),
  copyInvite: (provider: Provider) => emit("copyInvite", provider),
  copySecret: (
    provider: Provider,
    field: "apiKey" | "accessToken" | "sessionCookie",
  ) => emit("copySecret", provider, field),
  interaction: (active: boolean) => emit("interaction", active),
};
</script>

<template>
  <WorkspaceCardFooter
    :class="{ 'provider-card-footer-state': renewal.label }"
    :actions-visible="interactive"
  >
    <!-- 元信息位四类卡片同一读法：先是这张卡用什么凭据，再是它自己的异常或绑定。 -->
    <template #meta>
      <span
        class="provider-card-auth-summary"
        :title="authModeDescription"
        :aria-label="authModeDescription"
      >
        <ProviderAuthIcon
          :mode="provider.auth.mode"
          :protocol="provider.identity.protocol"
        />
      </span>
      <span
        v-if="renewal.label"
        class="provider-card-footer-note"
        :class="renewal.tone"
        :title="renewalTitle"
        >{{ renewal.label }}</span
      >
      <span
        v-else-if="boundPill"
        class="provider-card-footer-bound"
        :title="boundTitle"
        ><AgentCliIcon :kind="boundPill" :size="13" /><span
          >{{ boundLabel
          }}<template v-if="boundKinds.length > 1">
            +{{ boundKinds.length - 1 }}</template
          ></span
        ></span
      >
    </template>
    <WorkspaceCardActionGroup :label="`${noun}操作`">
      <CardIconButton
        v-if="canLaunchCli"
        tone="launch"
        :title="launchTitle"
        :aria-label="launchTitle"
        @click.stop="launchTemporaryCli"
        @pointerdown.stop
      >
        <AgentCliIcon v-if="launchCliKind" :kind="launchCliKind" :size="16" />
        <SquareTerminal v-else :size="15" :stroke-width="1.9" />
      </CardIconButton>

      <ProviderCheckInAction
        v-if="canCheckIn"
        :provider-id="provider.identity.id"
        :checked-in-today="checkedInToday"
        :checking-in="checkingIn"
        @check-in="checkInProvider"
        @interaction="emit('interaction', $event)"
      />

      <CardIconButton
        tone="refresh"
        :disabled="!canRefresh"
        :aria-busy="syncing"
        :title="refreshTitle"
        :aria-label="refreshTitle"
        @click="refreshProvider"
        @pointerdown.stop
      >
        <IconLoading v-if="syncing" />
        <IconRefresh v-else />
      </CardIconButton>

      <!-- 默认授权是每天都会动的动作，放在底栏。按钮只在「这个 Agent 正在用
           本卡授权」时显示 Agent 图标（状态），否则显示切换箭头——否则它会
           和左边的「启动 CLI」用同一枚 Agent 图标，两个按钮读起来一模一样。 -->
      <CardIconButton
        v-if="singleSwitchKind"
        :class="{ 'is-serving': isServing(singleSwitchKind) }"
        tone="configuration"
        :disabled="!canSwitchCliConfig"
        :title="switchTitle"
        :aria-label="switchTitle"
        @click.stop="switchCliConfig(singleSwitchKind)"
        @pointerdown.stop
      >
        <AgentCliIcon
          v-if="isServing(singleSwitchKind)"
          :kind="singleSwitchKind"
          :size="16"
        />
        <ArrowLeftRight v-else :size="15" :stroke-width="1.9" />
      </CardIconButton>
      <!-- 网关和中转站卡片都能把同一张卡喂给多个 Agent，这枚按钮的动作是
           「把这些 Agent 的默认授权换到本卡」，不是启动某一个 Agent。多目标时
           画其中一家的图标会被读成「点它会启动 Codex」，所以只画切换箭头；谁正
           在用本卡由 is-serving 的高亮和 title 说明，具体目标在菜单里逐条列出。 -->
      <a-dropdown
        v-if="switchControlVisible && !singleSwitchKind"
        trigger="click"
        position="br"
        :disabled="!canSwitchCliConfig"
        @popup-visible-change="emit('interaction', $event)"
      >
        <CardIconButton
          tone="configuration"
          :class="{ 'is-serving': Boolean(bound) }"
          :disabled="!canSwitchCliConfig"
          :title="switchTitle"
          :aria-label="switchTitle"
          @click.stop
          @pointerdown.stop
        >
          <ArrowLeftRight :size="15" :stroke-width="1.9" />
        </CardIconButton>
        <template #content>
          <a-doption
            v-for="kind in switchableKinds"
            :key="kind"
            @click="switchCliConfig(kind)"
          >
            <template #icon><AgentCliIcon :kind="kind" :size="15" /></template>
            <span class="provider-card-cli-option">
              <strong>{{ agentCliLabel(store.cliEnvironmentProbe, kind) }}</strong>
              <small>{{ cliOptionHint(kind) }}</small>
            </span>
          </a-doption>
        </template>
      </a-dropdown>

      <CardIconButton
        v-if="provider.actions.cpaGateway"
        :disabled="!provider.runtime.enabled"
        title="让网关里全部账号重新续期"
        aria-label="全部续期"
        @click.stop="emit('cpaRenewAll', provider)"
        @pointerdown.stop
      >
        <RotateCcw :size="15" :stroke-width="1.9" />
      </CardIconButton>

      <CardIconButton
        v-if="reauthRecommended"
        tone="warning"
        :title="reauthTitle"
        aria-label="重新授权"
        @click.stop="emit('reauthorizeOauthCard', provider)"
        @pointerdown.stop
      >
        <RotateCcw :size="15" :stroke-width="1.9" />
      </CardIconButton>

      <CardIconButton
        tone="edit"
        :title="`编辑${noun}`"
        :aria-label="`编辑${noun}`"
        @click="editProvider"
        @pointerdown.stop
      >
        <IconEdit />
      </CardIconButton>

      <CardIconButton
        :tone="provider.runtime.enabled ? 'warning' : 'success'"
        :title="provider.runtime.enabled ? `停用${noun}` : `启用${noun}`"
        :aria-label="provider.runtime.enabled ? `停用${noun}` : `启用${noun}`"
        @click="toggleProvider"
        @pointerdown.stop
      >
        <PowerOff
          v-if="provider.runtime.enabled"
          :size="15"
          :stroke-width="1.9"
        />
        <Power v-else :size="15" :stroke-width="1.9" />
      </CardIconButton>

      <CardIconButton
        tone="danger"
        :title="`删除${noun}`"
        :aria-label="`删除${noun}`"
        @click="removeProvider"
        @pointerdown.stop
      >
        <IconDelete />
      </CardIconButton>
    </WorkspaceCardActionGroup>

    <ProviderCardActionMenus
      :provider="provider"
      :probing-capabilities="probingCapabilities"
      v-on="menuListeners"
    />
  </WorkspaceCardFooter>
</template>
<style scoped>
/* The agent mark is also the switch target, so the agent that already uses
   this credential has to read as selected without relying on position. */
.card-icon-action.is-serving {
  background: color-mix(in srgb, currentColor 12%, transparent);
  box-shadow: inset 0 0 0 1px color-mix(in srgb, currentColor 28%, transparent);
}
.provider-card-cli-option {
  display: grid;
  gap: 1px;
  line-height: 1.3;
}
.provider-card-cli-option small {
  color: var(--color-text-3);
  font-size: 10px;
}

@media (prefers-reduced-motion: reduce) {
  .card-icon-action.is-serving {
    transition: none;
  }
}
</style>
