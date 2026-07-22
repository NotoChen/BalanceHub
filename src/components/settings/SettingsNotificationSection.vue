<script setup lang="ts">
import {
  IconDelete,
  IconNotification,
  IconPlus,
} from "@arco-design/web-vue/es/icon";
import type {
  AppSettings,
  NotificationChannel,
  NotificationChannelKind,
} from "../../stores/providers";

const props = defineProps<{
  settings: AppSettings;
  expanded?: boolean;
}>();

const emit = defineEmits<{
  toggle: [];
  "test-notification": [];
}>();

const channelKindOptions: { label: string; value: NotificationChannelKind }[] = [
  { label: "系统通知", value: "system" },
  { label: "钉钉", value: "dingtalk" },
  { label: "企业微信", value: "wecom" },
  { label: "飞书", value: "feishu" },
  { label: "Slack", value: "slack" },
  { label: "通用 Webhook", value: "generic" },
];

function addChannel() {
  const kind: NotificationChannelKind = "dingtalk";
  props.settings.notificationChannels.push({
    id: `notification-${Date.now()}-${Math.random().toString(36).slice(2, 8)}`,
    name: availableChannelName(kind),
    kind,
    url: "",
    secret: "",
    enabled: true,
  });
}

function removeChannel(channel: NotificationChannel) {
  props.settings.notificationChannels = props.settings.notificationChannels.filter(
    (item) => item.id !== channel.id,
  );
}

function updateChannelKind(channel: NotificationChannel, kind: NotificationChannelKind) {
  const oldDefaultName = channelKindLabel(channel.kind);
  const currentName = channel.name.trim();
  const suffix = currentName.slice(oldDefaultName.length).trim();
  const shouldUseDefaultName =
    !currentName || currentName === oldDefaultName ||
    (currentName.startsWith(`${oldDefaultName} `) && /^\d+$/.test(suffix));
  channel.kind = kind;
  if (shouldUseDefaultName) {
    channel.name = availableChannelName(kind, channel.id);
  }
  if (kind === "system") {
    channel.url = "";
    channel.secret = "";
  }
}

function channelKindLabel(kind: NotificationChannelKind) {
  return channelKindOptions.find((option) => option.value === kind)?.label || "通知渠道";
}

function availableChannelName(kind: NotificationChannelKind, excludeId?: string) {
  const baseName = channelKindLabel(kind);
  const usedNames = new Set(
    props.settings.notificationChannels
      .filter((channel) => channel.id !== excludeId)
      .map((channel) => channel.name.trim()),
  );
  if (!usedNames.has(baseName)) return baseName;

  let suffix = 2;
  while (usedNames.has(`${baseName} ${suffix}`)) suffix += 1;
  return `${baseName} ${suffix}`;
}

function channelNeedsSecret(kind: NotificationChannelKind) {
  return kind === "dingtalk" || kind === "feishu";
}
</script>

<template>
  <div class="settings-page settings-notification-page">
    <section class="settings-card settings-notification-master">
      <header class="settings-card-header">
        <span class="settings-card-icon"><IconNotification /></span>
        <div>
          <strong>通知中心</strong>
        </div>
        <a-button size="small" :disabled="!settings.notificationEnabled" @click="emit('test-notification')">
          发送测试
        </a-button>
      </header>
      <div class="settings-setting-row">
        <div class="settings-setting-copy">
          <strong>启用通知</strong>
        </div>
        <a-switch v-model="settings.notificationEnabled" />
      </div>
    </section>

    <section class="settings-card settings-channel-section" :class="{ disabled: !settings.notificationEnabled }">
      <header class="settings-card-header">
        <div>
          <strong>通知渠道</strong>
        </div>
        <span class="settings-card-state">{{ settings.notificationChannels.length }} 个</span>
        <a-button type="outline" size="small" @click="addChannel">
          <template #icon><IconPlus /></template>
          新增渠道
        </a-button>
      </header>

      <div class="notification-channel-list">
        <article
          v-for="channel in settings.notificationChannels"
          :key="channel.id"
          class="notification-channel"
          :class="{ disabled: !channel.enabled }"
        >
          <div class="notification-channel-header">
            <span class="notification-channel-status" :class="{ active: channel.enabled }" />
            <a-select
              class="notification-channel-kind-select"
              size="small"
              :model-value="channel.kind"
              :options="channelKindOptions"
              aria-label="渠道类型"
              @update:model-value="updateChannelKind(channel, $event as NotificationChannelKind)"
            />
            <a-switch v-model="channel.enabled" size="small" />
            <a-button
              v-if="channel.id !== 'system'"
              type="text"
              status="danger"
              aria-label="删除通知渠道"
              @click="removeChannel(channel)"
            >
              <template #icon><IconDelete /></template>
            </a-button>
          </div>
          <div
            class="notification-channel-fields"
            :class="{ 'has-secret': channelNeedsSecret(channel.kind) }"
          >
            <label v-if="channel.kind !== 'system'" class="notification-channel-field notification-channel-url-field">
              <span>Webhook 地址</span>
              <a-input
                v-model="channel.url"
                placeholder="https://"
                allow-clear
              />
            </label>
            <label v-if="channelNeedsSecret(channel.kind)" class="notification-channel-field">
              <span>签名密钥</span>
              <a-input-password
                v-model="channel.secret"
                placeholder="请输入密钥"
                allow-clear
              />
            </label>
          </div>
        </article>
      </div>
    </section>
  </div>
</template>
