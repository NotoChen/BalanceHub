<script setup lang="ts">
import SettingsSection from "./SettingsSection.vue";
import type {
  AppSettings,
  NotificationChannel,
  NotificationChannelKind,
} from "../../stores/providers";

const props = defineProps<{
  settings: AppSettings;
  expanded: boolean;
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
  props.settings.notificationChannels.push({
    id: `notification-${Date.now()}-${Math.random().toString(36).slice(2, 8)}`,
    name: "钉钉",
    kind: "dingtalk",
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
  const shouldUseDefaultName = !channel.name.trim() || channel.name === oldDefaultName;
  channel.kind = kind;
  if (shouldUseDefaultName) {
    channel.name = channelKindLabel(kind);
  }
  if (kind === "system") {
    channel.url = "";
    channel.secret = "";
  }
}

function channelKindLabel(kind: NotificationChannelKind) {
  return channelKindOptions.find((option) => option.value === kind)?.label || "通知渠道";
}
</script>

<template>
  <SettingsSection
    title="通知"
    description="统一管理系统通知和 Webhook 渠道。"
    :expanded="expanded"
    @toggle="emit('toggle')"
  >
    <a-form-item label="通知总开关">
      <div class="setting-action-row">
        <a-switch v-model="settings.notificationEnabled" />
        <a-button size="small" @click="emit('test-notification')">测试通知</a-button>
      </div>
    </a-form-item>
    <div class="notification-channel-list">
      <div
        v-for="channel in settings.notificationChannels"
        :key="channel.id"
        class="notification-channel"
      >
        <div class="notification-channel-header">
          <a-switch v-model="channel.enabled" />
          <a-input v-model="channel.name" placeholder="渠道名称" />
          <a-button
            v-if="channel.id !== 'system'"
            size="mini"
            status="danger"
            @click="removeChannel(channel)"
          >
            删除
          </a-button>
        </div>
        <a-select
          :model-value="channel.kind"
          :options="channelKindOptions"
          @update:model-value="updateChannelKind(channel, $event as NotificationChannelKind)"
        />
        <a-input
          v-if="channel.kind !== 'system'"
          v-model="channel.url"
          placeholder="Webhook 地址"
          allow-clear
        />
        <a-input-password
          v-if="channel.kind === 'dingtalk' || channel.kind === 'feishu'"
          v-model="channel.secret"
          placeholder="签名密钥"
          allow-clear
        />
      </div>
      <a-button type="outline" size="small" @click="addChannel">新增渠道</a-button>
    </div>
  </SettingsSection>
</template>
