<script setup lang="ts">
import { computed } from "vue";
import {
  IconDelete,
  IconEdit,
  IconLoading,
  IconRefresh,
} from "@arco-design/web-vue/es/icon";
import { CalendarCheck2, Power, PowerOff } from "@lucide/vue";
import ProviderAuthIcon from "../ProviderAuthIcon.vue";
import type { AgentCliKind, Provider } from "../../stores/providers";
import { providerAuthModeDescription } from "../../utils/provider-display";
import {
  providerCheckedInToday,
  supportsCheckIn,
} from "../../utils/provider-actions";
import type { CcSwitchAppTarget } from "../../utils/ccswitch-deeplink";
import ProviderCardActionMenus from "./ProviderCardActionMenus.vue";

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
  interaction: [active: boolean];
}>();

const authModeDescription = computed(() => providerAuthModeDescription(props.provider));
const canCheckInAction = computed(
  () => props.provider.runtime.enabled && supportsCheckIn(props.provider),
);
const checkedInToday = computed(() => providerCheckedInToday(props.provider));
const refreshActionTitle = computed(() =>
  props.provider.actions.refreshModelsOnly ? "刷新模型列表" : "刷新额度",
);

const menuListeners = {
  switchCliConfig: (provider: Provider, cliKind: AgentCliKind) =>
    emit("switchCliConfig", provider, cliKind),
  probeCapabilities: (provider: Provider) => emit("probeCapabilities", provider),
  openApiKeyManager: (provider: Provider) => emit("openApiKeyManager", provider),
  openAvailableModels: (provider: Provider) => emit("openAvailableModels", provider),
  openUsage: (provider: Provider) => emit("openUsage", provider),
  openRequestLogs: (provider: Provider) => emit("openRequestLogs", provider),
  openPasswordChange: (provider: Provider) => emit("openPasswordChange", provider),
  openLivenessDetails: (provider: Provider) => emit("openLivenessDetails", provider),
  openCheckInRecords: (provider: Provider) => emit("openCheckInRecords", provider),
  addCcSwitchConfig: (provider: Provider, target: CcSwitchAppTarget) =>
    emit("addCcSwitchConfig", provider, target),
  launchTemporaryCli: (provider: Provider) => emit("launchTemporaryCli", provider),
  copyUrl: (provider: Provider) => emit("copyUrl", provider),
  copyInvite: (provider: Provider) => emit("copyInvite", provider),
  copySecret: (
    provider: Provider,
    field: "apiKey" | "accessToken" | "sessionCookie",
  ) => emit("copySecret", provider, field),
  interaction: (active: boolean) => emit("interaction", active),
};

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

      <ProviderCardActionMenus
        :provider="provider"
        :switching-cli-kind="switchingCliKind"
        :cli-config-switching="cliConfigSwitching"
        :probing-capabilities="probingCapabilities"
        v-on="menuListeners"
      />
    </div>
  </footer>
</template>
