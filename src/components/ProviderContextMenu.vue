<script setup lang="ts">
import { computed } from "vue";
import {
  IconCheckCircleFill,
  IconCopy,
  IconDelete,
  IconEdit,
  IconLink,
  IconPauseCircleFill,
  IconPlayCircleFill,
  IconRefresh,
  IconRight,
  IconSettings,
  IconSync,
} from "@arco-design/web-vue/es/icon";
import type { Provider } from "../stores/providers";
import {
  providerCheckedInToday,
  supportsApiKeyManagement,
  supportsCheckIn,
  supportsInvitation,
} from "../utils/provider-display";

const props = defineProps<{
  provider: Provider;
  x: number;
  y: number;
  checkingIn: boolean;
  syncingCapabilities: boolean;
  testingLiveness: boolean;
}>();

const emit = defineEmits<{
  toggle: [provider: Provider];
  refresh: [provider: Provider];
  syncCapabilities: [provider: Provider];
  testLiveness: [provider: Provider];
  edit: [provider: Provider];
  checkIn: [provider: Provider];
  openApiKeyManager: [provider: Provider];
  openUsage: [provider: Provider];
  openRequestLogs: [provider: Provider];
  openPasswordChange: [provider: Provider];
  openLivenessDetails: [provider: Provider];
  openCheckInRecords: [provider: Provider];
  copyUrl: [provider: Provider];
  copyInvite: [provider: Provider];
  copySecret: [provider: Provider, field: "apiKey" | "accessToken" | "sessionCookie"];
  remove: [provider: Provider];
}>();

const hasCopyActions = computed(
  () =>
    Boolean(
      props.provider.identity.baseUrl.trim() ||
        props.provider.auth.apiKey.trim() ||
        props.provider.auth.accessToken.trim() ||
        props.provider.auth.sessionCookie.trim() ||
        (props.provider.runtime.enabled && supportsInvitation(props.provider)),
    ),
);

const hasSiteActions = computed(
  () =>
    Boolean(
      supportsApiKeyManagement(props.provider) ||
        (props.provider.auth.apiUser.trim() &&
          (props.provider.auth.accessToken.trim() || props.provider.auth.sessionCookie.trim())) ||
        props.provider.runtime.enabled,
    ),
);

const submenuAlignLeft = computed(() => props.x > window.innerWidth - 440);
</script>

<template>
  <div
    class="provider-context-menu"
    :style="{ left: `${x}px`, top: `${y}px` }"
    @click.stop
    @contextmenu.prevent
  >
    <div class="provider-context-menu-group">
      <button
        v-if="provider.runtime.enabled && supportsCheckIn(provider)"
        type="button"
        :disabled="providerCheckedInToday(provider) || checkingIn"
        @click="emit('checkIn', provider)"
      >
        <icon-check-circle-fill />
        <span>{{ providerCheckedInToday(provider) ? "已签到" : "签到" }}</span>
      </button>
      <button type="button" :disabled="!provider.runtime.enabled" @click="emit('refresh', provider)">
        <icon-refresh />
        <span>刷新额度</span>
      </button>
      <button
        type="button"
        :disabled="!provider.runtime.enabled || !provider.auth.apiKey.trim() || testingLiveness"
        @click="emit('testLiveness', provider)"
      >
        <icon-sync />
        <span>{{ testingLiveness ? "测活中" : "测活" }}</span>
      </button>
    </div>

    <div class="provider-context-menu-group">
      <div
        v-if="hasCopyActions"
        class="provider-context-menu-submenu"
        :class="{ 'provider-context-menu-submenu-left': submenuAlignLeft }"
      >
        <button type="button" class="provider-context-menu-submenu-trigger" aria-haspopup="menu">
          <icon-copy />
          <span>复制</span>
          <icon-right class="provider-context-menu-arrow" />
        </button>
        <div class="provider-context-menu-submenu-panel" role="menu">
          <button v-if="provider.identity.baseUrl.trim()" type="button" @click="emit('copyUrl', provider)">
            <icon-link />
            <span>中转站 URL</span>
          </button>
          <button
            v-if="provider.auth.apiKey.trim()"
            type="button"
            @click="emit('copySecret', provider, 'apiKey')"
          >
            <icon-copy />
            <span>API 密钥</span>
          </button>
          <button
            v-if="provider.auth.accessToken.trim()"
            type="button"
            @click="emit('copySecret', provider, 'accessToken')"
          >
            <icon-copy />
            <span>访问令牌</span>
          </button>
          <button
            v-if="provider.auth.sessionCookie.trim()"
            type="button"
            @click="emit('copySecret', provider, 'sessionCookie')"
          >
            <icon-copy />
            <span>Cookie</span>
          </button>
          <button
            v-if="provider.runtime.enabled && supportsInvitation(provider)"
            type="button"
            :disabled="syncingCapabilities"
            @click="emit('copyInvite', provider)"
          >
            <icon-link />
            <span>邀请链接</span>
          </button>
        </div>
      </div>

      <div
        class="provider-context-menu-submenu"
        :class="{ 'provider-context-menu-submenu-left': submenuAlignLeft }"
      >
        <button type="button" class="provider-context-menu-submenu-trigger" aria-haspopup="menu">
          <icon-refresh />
          <span>数据</span>
          <icon-right class="provider-context-menu-arrow" />
        </button>
        <div class="provider-context-menu-submenu-panel" role="menu">
          <button type="button" @click="emit('openUsage', provider)">
            <icon-refresh />
            <span>用量趋势</span>
          </button>
          <button type="button" @click="emit('openRequestLogs', provider)">
            <icon-refresh />
            <span>请求日志</span>
          </button>
          <button type="button" @click="emit('openLivenessDetails', provider)">
            <icon-sync />
            <span>测活明细</span>
          </button>
          <button type="button" @click="emit('openCheckInRecords', provider)">
            <icon-check-circle-fill />
            <span>签到记录</span>
          </button>
        </div>
      </div>

      <div
        v-if="hasSiteActions"
        class="provider-context-menu-submenu"
        :class="{ 'provider-context-menu-submenu-left': submenuAlignLeft }"
      >
        <button type="button" class="provider-context-menu-submenu-trigger" aria-haspopup="menu">
          <icon-settings />
          <span>站点</span>
          <icon-right class="provider-context-menu-arrow" />
        </button>
        <div class="provider-context-menu-submenu-panel" role="menu">
          <button
            type="button"
            :disabled="!provider.runtime.enabled || syncingCapabilities"
            @click="emit('syncCapabilities', provider)"
          >
            <icon-sync />
            <span>同步站点能力</span>
          </button>
          <button
            v-if="supportsApiKeyManagement(provider)"
            type="button"
            @click="emit('openApiKeyManager', provider)"
          >
            <icon-settings />
            <span>密钥管理</span>
          </button>
          <button
            v-if="provider.auth.apiUser.trim() && (provider.auth.accessToken.trim() || provider.auth.sessionCookie.trim())"
            type="button"
            @click="emit('openPasswordChange', provider)"
          >
            <icon-settings />
            <span>修改密码</span>
          </button>
        </div>
      </div>
    </div>

    <div class="provider-context-menu-group">
      <button type="button" @click="emit('edit', provider)">
        <icon-edit />
        <span>编辑</span>
      </button>
      <button type="button" @click="emit('toggle', provider)">
        <icon-pause-circle-fill v-if="provider.runtime.enabled" />
        <icon-play-circle-fill v-else />
        <span>{{ provider.runtime.enabled ? "停用" : "启用" }}</span>
      </button>
      <button type="button" class="danger" @click="emit('remove', provider)">
        <icon-delete />
        <span>删除</span>
      </button>
    </div>
  </div>
</template>
